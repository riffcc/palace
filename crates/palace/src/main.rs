//! Palace: AI-assisted software development with gamepad control.
//!
//! This is the main binary that ties together:
//! - Mountain: Time-delayed cascading LLM control
//! - Conductor: Recursive interview UI with gamepad
//! - Director: Autonomous project management
//! - Palace-GBA: GBA emulation integration
//! - Palace-render: Dual-screen wgpu rendering
//!
//! # Usage
//!
//! ```bash
//! # Play a GBA ROM
//! palace play --rom /path/to/pokemon.gba --bios /path/to/bios.bin
//!
//! # Run in project mode (AI-assisted development)
//! palace project --path /path/to/project
//! ```

mod app;
mod config;
mod gamepad;
mod gba_controllable;
mod inference;

use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::info;

#[derive(Parser)]
#[command(name = "palace")]
#[command(about = "AI-assisted software development with gamepad control")]
#[command(version)]
struct Cli {
    /// LM Studio endpoint URL (uses Z.ai API if not provided)
    #[arg(long)]
    lm_studio: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Play a GBA ROM with AI assistance
    Play {
        /// Path to GBA ROM file
        #[arg(long)]
        rom: PathBuf,

        /// Path to GBA BIOS
        #[arg(long)]
        bios: PathBuf,

        /// Enable turbo mode (uncapped framerate)
        #[arg(long)]
        turbo: bool,
    },

    /// AI-assisted project development
    Project {
        /// Path to project directory
        #[arg(long, default_value = ".")]
        path: PathBuf,

        /// Project name
        #[arg(long)]
        name: Option<String>,
    },

    /// Show connected gamepads
    Gamepads,

    /// Test LM Studio connection
    TestLlm {
        /// Prompt to send
        #[arg(default_value = "Hello, what model are you?")]
        prompt: String,
    },

    /// Generate suggestions for what to do next
    Next {
        /// Project directory (defaults to current)
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// List pending or active tasks
    Ls {
        /// Show active tasks from Plane.so instead of pending
        #[arg(long)]
        active: bool,

        /// Project directory (defaults to current)
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Remove pending tasks by number
    Rm {
        /// Task numbers to remove (e.g., 1,4,5,8,2)
        numbers: String,

        /// Project directory (defaults to current)
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Approve pending tasks and create Plane.so issues
    Approve {
        /// Task numbers to approve (e.g., 1,2,3,5,6)
        numbers: String,

        /// Project directory (defaults to current)
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Call a Palace tool directly (for Claude Code integration)
    Call {
        /// Tool name: create-issue, list-issues, update-issue
        tool: String,

        /// JSON input for the tool
        #[arg(long)]
        input: Option<String>,

        /// Project directory (defaults to current)
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// Plane.so workspace slug (auto-init if needed)
        #[arg(long)]
        workspace: Option<String>,

        /// Plane.so project slug (auto-init if needed)
        #[arg(long)]
        project: Option<String>,
    },

    /// Run the Palace daemon (listens for @palace commands via Zulip)
    /// One daemon handles unlimited projects - project context from Zulip stream/topic.
    Daemon,

    /// Manage agent sessions
    #[command(subcommand)]
    Session(SessionCommands),

    /// Run the Director daemon (autonomous project manager)
    Director {
        /// Node name (machine identifier, e.g., "tealc")
        #[arg(short, long)]
        node: String,

        /// Director specs: name:model pairs (e.g., "alpha:flash", "beta:lm:/qwen3-8b")
        /// Model prefixes: lm:/ (LM Studio), z:/ (Z.ai), or:/ (OpenRouter), ms:/ (Mistral)
        #[arg(required = true)]
        directors: Vec<String>,

        /// Zulip stream to use
        #[arg(long, default_value = "palace")]
        stream: String,

        /// Verbosity level: quiet, normal, verbose
        #[arg(long, default_value = "normal")]
        verbosity: String,
    },

    /// Run SWE-bench benchmark
    Bench {
        /// Dataset variant: lite, full, verified
        #[arg(long, default_value = "lite")]
        dataset: String,

        /// Limit number of instances to run
        #[arg(long)]
        limit: Option<usize>,

        /// LLM endpoint URL (uses Z.ai API if not provided)
        #[arg(long)]
        llm_url: Option<String>,

        /// Model name
        #[arg(long, default_value = "GLM-4.7")]
        model: String,

        /// Timeout per instance in seconds
        #[arg(long, default_value = "900")]
        timeout: u64,

        /// Number of parallel workers (default: 12)
        #[arg(long, short = 'j', default_value = "12")]
        parallel: usize,

        /// Output file for predictions (JSONL)
        #[arg(long)]
        output: Option<PathBuf>,

        /// Local JSONL file instead of HuggingFace
        #[arg(long)]
        file: Option<PathBuf>,

        /// Working directory for cloned repos
        #[arg(long, default_value = "/opt/swebench")]
        work_dir: PathBuf,
    },

    /// Race variants of a single SWE-bench instance to find optimal approach
    Race {
        /// Instance ID to race (e.g., "django__django-10914")
        instance_id: String,

        /// Number of variants to run
        #[arg(long, short = 'n', default_value = "5")]
        variants: usize,

        /// LLM endpoint URL
        #[arg(long)]
        llm_url: Option<String>,

        /// Model name
        #[arg(long, default_value = "GLM-4.7")]
        model: String,

        /// Working directory for cloned repos
        #[arg(long, default_value = "/opt/swebench")]
        work_dir: PathBuf,

        /// Output directory for race results
        #[arg(long, default_value = "/tmp/palace-race")]
        output_dir: PathBuf,

        /// Analyze existing trace file instead of running
        #[arg(long)]
        analyze: Option<PathBuf>,
    },

    /// Evaluate SWE-bench predictions using official harness
    Eval {
        /// Predictions file (JSONL)
        predictions: PathBuf,

        /// Dataset variant: lite, full, verified
        #[arg(long, default_value = "lite")]
        dataset: String,

        /// Number of parallel workers (default: 75% of CPU cores, max 12)
        #[arg(long)]
        workers: Option<usize>,

        /// Run ID for this evaluation
        #[arg(long)]
        run_id: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    /// List active sessions
    Ls {
        /// Show all sessions (including completed)
        #[arg(short, long)]
        all: bool,

        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// View session logs
    Log {
        /// Session ID or name
        session: String,

        /// Follow log output (like tail -f)
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show
        #[arg(short, long)]
        lines: Option<usize>,

        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Create a new session
    Create {
        /// Target: issue ID (PAL-123), module name, or cycle name
        target: String,

        /// Execution strategy: simple, parallel, priority
        #[arg(short, long, default_value = "simple")]
        strategy: String,

        /// Run immediately after creation
        #[arg(short, long)]
        run: bool,

        /// Run in background (implies --run)
        #[arg(short, long)]
        background: bool,

        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// LM Studio URL
        #[arg(long, default_value = "http://localhost:1234/v1")]
        llm_url: String,

        /// Model to use
        #[arg(long, default_value = "glm-4-plus")]
        model: String,
    },

    /// Cancel a session
    Cancel {
        /// Session ID or name
        session: String,

        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Show session details
    Info {
        /// Session ID or name
        session: String,

        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Run an existing session
    Run {
        /// Session ID or name
        session: String,

        /// Project directory
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// LM Studio URL
        #[arg(long, default_value = "http://localhost:1234/v1")]
        llm_url: String,

        /// Model to use
        #[arg(long, default_value = "glm-4-plus")]
        model: String,
    },
}

fn main() -> anyhow::Result<()> {
    // Load .env from ~/.palace/.env first, then current directory
    if let Ok(home) = std::env::var("HOME") {
        let palace_env = std::path::PathBuf::from(home).join(".palace").join(".env");
        dotenvy::from_path(palace_env).ok();
    }
    // Current directory .env can override
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    // Initialize logging - respect RUST_LOG env var, with sensible defaults
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        if cli.verbose {
            "palace=debug,palace_plane=debug,palace_gba=debug,mountain=debug,conductor=debug,director=debug,llm_code_sdk=debug".to_string()
        } else {
            "palace=info,palace_plane=info,palace_gba=info,mountain=info,conductor=info,director=info".to_string()
        }
    });

    tracing_subscriber::fmt()
        .with_env_filter(&filter)
        .init();

    info!("Palace starting...");

    match cli.command {
        Commands::Play { rom, bios, turbo } => {
            let lm_url = cli.lm_studio.as_deref().unwrap_or("http://localhost:1234/v1");
            app::run_play(rom, bios, turbo, lm_url)?;
        }

        Commands::Project { path, name } => {
            let lm_url = cli.lm_studio.as_deref().unwrap_or("http://localhost:1234/v1");
            app::run_project(path, name, lm_url)?;
        }

        Commands::Gamepads => {
            gamepad::list_gamepads()?;
        }

        Commands::TestLlm { prompt } => {
            let rt = tokio::runtime::Runtime::new()?;
            let lm_url = cli.lm_studio.as_deref().unwrap_or("http://localhost:1234/v1");
            rt.block_on(inference::test_connection(lm_url, &prompt))?;
        }

        Commands::Next { path } => {
            let rt = tokio::runtime::Runtime::new()?;

            // Print welcome banner
            println!("\n\x1b[1;36m🏛️  Palace\x1b[0m\n");
            println!("\x1b[33m📡 Exploring your codebase...\x1b[0m");

            // Setup Zulip for real-time streaming
            let palace_dir = path.join(".palace");
            let zulip_stream = match palace_plane::ProjectConfig::load(&path) {
                Ok(config) => config.name.unwrap_or_else(|| "palace".to_string()),
                Err(_) => {
                    eprintln!("\x1b[33m⚠️  No .palace/project.yml found. Run 'pal init' to configure.\x1b[0m");
                    "palace".to_string()
                }
            };
            let zulip_topic = "suggestions";

            eprintln!("[ZULIP] stream={} topic={}", zulip_stream, zulip_topic);

            // Try to get Zulip tool and post initial message
            let zulip_state: Option<(director::ZulipTool, u64, String)> =
                match director::ZulipTool::from_env_palace() {
                    Ok(tool) => {
                        eprintln!("[ZULIP] tool created");
                        if let Err(e) = rt.block_on(tool.ensure_stream(&zulip_stream)) {
                            eprintln!("[ZULIP] ensure_stream failed: {}", e);
                        }
                        let initial_msg = "🔍 **Exploring codebase...**\n\n".to_string();
                        match rt.block_on(tool.send(&zulip_stream, zulip_topic, &initial_msg)) {
                            Ok(msg_id) => {
                                eprintln!("[ZULIP] posted id={}", msg_id);
                                Some((tool, msg_id, initial_msg))
                            }
                            Err(e) => {
                                eprintln!("[ZULIP] send failed: {}", e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[ZULIP] from_env_palace failed: {}", e);
                        None
                    }
                };

            // Channel for non-blocking Zulip updates
            let (zulip_tx, mut zulip_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let zulip_tx_for_callback = zulip_tx.clone();

            // Spawn SEPARATE THREAD for Zulip updates (can't use rt.spawn - it blocks)
            let zulip_handle = if let Some((tool, msg_id, initial_content)) = zulip_state {
                Some(std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let mut content = initial_content;
                    while let Some(line) = rt.block_on(zulip_rx.recv()) {
                        content.push_str(&format!("{}\n", line));
                        if let Err(e) = rt.block_on(tool.update_message(msg_id, &content)) {
                            eprintln!("[ZULIP] update failed: {}", e);
                        }
                    }
                    (tool, msg_id, content)
                }))
            } else {
                None
            };

            // Create callback that streams to terminal and sends to Zulip channel
            let callback = {
                std::sync::Arc::new(move |event: palace_plane::ExplorationEvent| {
                    match event {
                        palace_plane::ExplorationEvent::ToolCall { name, input } => {
                            let line = match name.as_str() {
                                "read_file" => {
                                    input.get("path").and_then(|v| v.as_str())
                                        .map(|p| format!("📖 {}", p))
                                }
                                "list_directory" => {
                                    let p = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                                    Some(format!("📂 {}", p))
                                }
                                "grep" => {
                                    input.get("pattern").and_then(|v| v.as_str())
                                        .map(|p| format!("🔍 {}", p))
                                }
                                "glob" => {
                                    input.get("pattern").and_then(|v| v.as_str())
                                        .map(|p| format!("🔎 {}", p))
                                }
                                "suggest" => Some("💡 Generating...".to_string()),
                                _ => None,
                            };

                            if let Some(line) = line {
                                println!("\x1b[90m{}\x1b[0m", line);
                                let _ = zulip_tx_for_callback.send(line);
                            }
                        }
                        _ => {}
                    }
                })
            };

            let tasks = rt.block_on(palace_plane::generate_suggestions_with_options(
                &path,
                Some(callback),
                None, // request_up_to - let model decide
                cli.lm_studio.as_deref(),
            ))?;

            // Drop sender to signal background thread to finish, then wait for it
            drop(zulip_tx);
            let zulip_final = zulip_handle.and_then(|h| h.join().ok());

            println!();
            if tasks.is_empty() {
                println!("No suggestions generated.");
                if let Some((tool, msg_id, _)) = &zulip_final {
                    let _ = rt.block_on(tool.update_message(*msg_id, "No suggestions generated."));
                }
            } else {
                // Show raw suggestions in terminal
                for stored in &tasks {
                    println!("{}. {}", stored.index, stored.task.title);
                }

                // Convert for formatting
                let tasks_for_zulip: Vec<(usize, palace_plane::PendingTask)> = tasks
                    .iter()
                    .map(|s| (s.index, s.task.clone()))
                    .collect();

                // Update the streaming Zulip message with final suggestions
                if let Some((tool, msg_id, _)) = &zulip_final {
                    let merged = format_merged_suggestions(&tasks_for_zulip);
                    if let Err(e) = rt.block_on(tool.update_message(*msg_id, &merged)) {
                        eprintln!("\x1b[33m⚠️  Failed to update Zulip: {}\x1b[0m", e);
                    }

                    // Post poll as separate message
                    let poll = format_suggestions_poll(&tasks_for_zulip);
                    if let Err(e) = rt.block_on(tool.send(&zulip_stream, "suggestions", &poll)) {
                        eprintln!("\x1b[33m⚠️  Failed to send poll: {}\x1b[0m", e);
                    }

                    println!("\x1b[32m✓ Streamed {} suggestions to Zulip\x1b[0m\n", tasks.len());
                }

                // Progressive enhancement phase (silent from Zulip perspective)
                println!("\x1b[33m🔮 Enhancing suggestions with plans and relations...\x1b[0m\n");

                // Collect all enhanced tasks
                let enhanced = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
                let enhanced_clone = enhanced.clone();

                let enhancement_callback = {
                    std::sync::Arc::new(move |index: usize, task: &palace_plane::PendingTask| {
                        // Print to terminal
                        println!("\x1b[36m━━━ #{} {} ━━━\x1b[0m", index, task.title);

                        if let Some(plan) = &task.plan {
                            println!("\x1b[90mPlan:\x1b[0m");
                            for (i, step) in plan.iter().enumerate() {
                                println!("  {}. {}", i + 1, step);
                            }
                        }

                        if let Some(subtasks) = &task.subtasks {
                            if !subtasks.is_empty() {
                                println!("\x1b[90mSubtasks:\x1b[0m");
                                for st in subtasks {
                                    println!("  • {}", st);
                                }
                            }
                        }

                        if let Some(relations) = &task.relations {
                            if !relations.is_empty() {
                                println!("\x1b[90mRelations:\x1b[0m");
                                for rel in relations {
                                    let rel_type = match rel.relation_type {
                                        palace_plane::RelationType::DependsOn => "depends on",
                                        palace_plane::RelationType::Blocks => "blocks",
                                        palace_plane::RelationType::RelatedTo => "related to",
                                    };
                                    let reason = rel.reason.as_deref().unwrap_or("");
                                    println!("  → {} #{}{}", rel_type, rel.target_index,
                                        if reason.is_empty() { String::new() } else { format!(" ({})", reason) });
                                }
                            }
                        }
                        println!();

                        // Collect for batch Zulip post
                        enhanced_clone.lock().unwrap().push((index, task.clone()));
                    })
                };

                let lm_url = cli.lm_studio.clone();
                rt.block_on(palace_plane::enhance_all_suggestions(
                    &path,
                    Some(enhancement_callback),
                    lm_url.as_deref(),
                ))?;

                println!("\x1b[90mUse 'pal approve <n>' or '@palace approve 1,2,3' in Zulip.\x1b[0m");
            }
        }

        Commands::Ls { active, path } => {
            let rt = tokio::runtime::Runtime::new()?;
            if active {
                let config = palace_plane::ProjectConfig::load(&path)?;
                let tasks = rt.block_on(palace_plane::list_active(&path))?;

                if tasks.is_empty() {
                    println!("No active tasks in Plane.so.");
                } else {
                    println!("Active tasks in {}/{} ({}):\n",
                        config.workspace, config.project_slug, tasks.len());
                    for task in &tasks {
                        println!("  [{}-{}] {}",
                            config.project_slug.to_uppercase(), task.sequence_id, task.name);
                    }
                }
            } else {
                let suggestions = palace_plane::load_suggestions(&path)?;

                if suggestions.is_empty() {
                    println!("No pending suggestions. Run 'pal next' to generate some.");
                } else {
                    println!();
                    for s in &suggestions {
                        println!("{}. {}", s.index, s.task.title);
                        if let Some(desc) = &s.task.description {
                            if let Some(first_line) = desc.lines().next() {
                                println!("   \x1b[90m{}\x1b[0m", first_line);
                            }
                        }
                    }
                    println!();
                    println!("\x1b[90mUse 'pal approve <n>' to create issues, 'pal rm <n>' to remove.\x1b[0m");
                }
            }
        }

        Commands::Rm { numbers, path } => {
            let indices = parse_numbers(&numbers)?;
            let removed = palace_plane::remove_suggestions(&path, &indices)?;

            if removed.is_empty() {
                println!("No suggestions removed.");
            } else {
                println!("Removed {} suggestion(s):", removed.len());
                for s in &removed {
                    println!("  {}. {}", s.index, s.task.title);
                }
            }
        }

        Commands::Approve { numbers, path } => {
            let rt = tokio::runtime::Runtime::new()?;
            let config = palace_plane::ProjectConfig::load(&path)?;
            let indices = parse_numbers(&numbers)?;
            let results = rt.block_on(palace_plane::approve_tasks(&path, &indices))?;

            if results.is_empty() {
                println!("No tasks approved.");
            } else {
                println!("Approved {} task(s):\n", results.len());
                for (task, issue) in &results {
                    println!("  [{}-{}] {}",
                        config.project_slug.to_uppercase(), issue.sequence_id, task.title);
                }
            }
        }

        Commands::Call { tool, input, path, workspace, project } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(handle_call(&tool, input.as_deref(), &path, workspace.as_deref(), project.as_deref()))?;
        }

        Commands::Daemon => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                println!("🏛️  Palace Daemon starting...");
                println!("   One daemon, unlimited projects");
                println!("   Project context from Zulip stream/topic");

                // Create and run the Zulip reactor
                let mut reactor = director::ZulipReactor::from_env()
                    .map_err(|e| anyhow::anyhow!("Failed to create Zulip reactor: {}", e))?;

                println!("   Listening for @palace commands on Zulip");
                println!("\n🟢 Ready. Ctrl+C to shutdown.\n");

                reactor.run().await
                    .map_err(|e| anyhow::anyhow!("Reactor error: {}", e))?;

                Ok::<(), anyhow::Error>(())
            })?;
        }

        Commands::Session(cmd) => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(handle_session_command(cmd))?;
        }

        Commands::Director { node, directors, stream, verbosity } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                // Parse verbosity
                let verbosity = match verbosity.as_str() {
                    "quiet" => director::Verbosity::Quiet,
                    "verbose" => director::Verbosity::Verbose,
                    "debug" => director::Verbosity::Debug,
                    _ => director::Verbosity::Normal,
                };

                // Create pool
                let mut pool = director::Pool::new(&node)
                    .map_err(|e| anyhow::anyhow!("{}", e))?
                    .with_verbosity(verbosity)
                    .with_stream(&stream);

                // Add directors
                for spec in &directors {
                    let (name, model) = if let Some((n, m)) = spec.split_once(':') {
                        (n.to_string(), m.to_string())
                    } else {
                        (spec.clone(), "flash".to_string())
                    };
                    pool.add(&name, &model)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    info!("Added director '{}' with model '{}'", name, model);
                }

                // Run the pool
                let project_path = std::env::current_dir()?;
                pool.run(project_path).await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                Ok::<(), anyhow::Error>(())
            })?;
        }

        Commands::Bench { dataset, limit, llm_url, model, timeout, parallel, output, file, work_dir } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                use director::{SWEBenchLoader, SWEBenchRunner, BenchmarkConfig, DatasetVariant};
                use std::time::Duration;

                println!("🏛️  Palace SWE-bench Runner\n");

                // Parse dataset variant
                let variant = match dataset.as_str() {
                    "full" => DatasetVariant::Full,
                    "verified" => DatasetVariant::Verified,
                    _ => DatasetVariant::Lite,
                };

                // Load instances
                let instances = if let Some(path) = file {
                    println!("📁 Loading from: {}", path.display());
                    SWEBenchLoader::from_file(path)
                        .map_err(|e| anyhow::anyhow!("{}", e))?
                } else {
                    println!("🤗 Loading {} dataset from HuggingFace...", dataset);
                    SWEBenchLoader::from_huggingface(variant, limit)
                        .map_err(|e| anyhow::anyhow!("{}", e))?
                };

                let instance_count = if let Some(n) = limit {
                    instances.len().min(n)
                } else {
                    instances.len()
                };
                println!("   {} instances loaded\n", instance_count);

                // Determine LLM URL
                let llm_url = llm_url.unwrap_or_else(|| {
                    if std::env::var("ZAI_API_KEY").is_ok() {
                        "https://api.z.ai/api/anthropic".to_string()
                    } else {
                        "http://localhost:1234/v1".to_string()
                    }
                });

                // Create runner
                let config = BenchmarkConfig {
                    llm_url: llm_url.clone(),
                    api_key: std::env::var("ZAI_API_KEY").ok(),
                    model: model.clone(),
                    timeout: Duration::from_secs(timeout),
                    max_tokens: 65536,
                    work_dir,
                };

                println!("🤖 Model: {}", model);
                println!("🌐 Endpoint: {}", llm_url);
                println!("⏱️  Timeout: {}s per instance", timeout);
                if parallel > 1 {
                    println!("🚀 Parallel workers: {}", parallel);
                }
                println!();

                let mut runner = SWEBenchRunner::new(config);

                // Run instances
                let instances_to_run = if let Some(n) = limit {
                    &instances[..instances.len().min(n)]
                } else {
                    &instances[..]
                };

                let summary = if parallel > 1 {
                    runner.run_batch_parallel(instances_to_run, parallel).await
                } else {
                    runner.run_batch(instances_to_run).await
                };

                // Print summary
                println!("\n═══════════════════════════════════════════════════════");
                println!("📊 Results: {}/{} successful ({:.1}%)",
                    summary.success,
                    summary.total,
                    (summary.success as f64 / summary.total as f64) * 100.0
                );
                println!("⏱️  Total: {:.1}s, Average: {:.1}s per instance",
                    summary.total_duration.as_secs_f32(),
                    summary.avg_duration.as_secs_f32()
                );
                println!("═══════════════════════════════════════════════════════\n");

                // Write predictions - always save to default location
                let output_path = output.unwrap_or_else(|| {
                    PathBuf::from(format!("/tmp/swebench-palace/predictions_{}.jsonl",
                        chrono::Utc::now().format("%Y%m%d_%H%M%S")))
                });

                runner.write_predictions(&output_path, &model)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                println!("📝 Predictions written to: {}", output_path.display());

                // Show how to evaluate
                if summary.success > 0 {
                    println!("\n💡 To evaluate with official harness:");
                    println!("   pal eval {} --dataset {}", output_path.display(), dataset);
                }

                Ok::<(), anyhow::Error>(())
            })?;
        }

        Commands::Race { instance_id, variants, llm_url, model, work_dir, output_dir, analyze } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                use director::{TraceEvent, analyze_trace_for_smartread};

                println!("🏎️  Palace Race Mode\n");
                println!("Instance: {}", instance_id);

                // If analyzing existing trace
                if let Some(trace_path) = analyze {
                    println!("📊 Analyzing trace: {}\n", trace_path.display());

                    let trace_content = std::fs::read_to_string(&trace_path)?;
                    let events: Vec<TraceEvent> = trace_content
                        .lines()
                        .filter_map(|l| serde_json::from_str(l).ok())
                        .collect();

                    let analysis = analyze_trace_for_smartread(&events);
                    println!("{}", analysis);

                    return Ok::<(), anyhow::Error>(());
                }

                // TODO: Implement variant racing
                // For now, just run the instance once and analyze
                println!("🚧 Variant racing not yet implemented");
                println!("   Use --analyze <trace.jsonl> to analyze existing traces");

                Ok::<(), anyhow::Error>(())
            })?;
        }

        Commands::Eval { predictions, dataset, workers, run_id } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                use director::{run_swebench_evaluation, DatasetVariant};

                println!("🏛️  Palace SWE-bench Evaluator\n");

                // Parse dataset variant
                let variant = match dataset.as_str() {
                    "full" => DatasetVariant::Full,
                    "verified" => DatasetVariant::Verified,
                    _ => DatasetVariant::Lite,
                };

                // Calculate workers (75% of cores, max 12)
                let num_workers = workers.unwrap_or_else(|| {
                    let cores = num_cpus::get();
                    ((cores as f64 * 0.75) as usize).min(12).max(1)
                });

                println!("📁 Predictions: {}", predictions.display());
                println!("📊 Dataset: {}", dataset);
                println!("🔧 Workers: {}\n", num_workers);

                let result = run_swebench_evaluation(
                    &predictions,
                    variant,
                    num_workers,
                    run_id.as_deref(),
                ).await;

                match result {
                    Ok(output) => {
                        println!("✅ Evaluation complete!\n");
                        println!("{}", output);
                    }
                    Err(e) => {
                        eprintln!("❌ Evaluation failed: {}", e);
                        std::process::exit(1);
                    }
                }

                Ok::<(), anyhow::Error>(())
            })?;
        }
    }

    Ok(())
}

/// Handle session subcommands.
async fn handle_session_command(cmd: SessionCommands) -> anyhow::Result<()> {
    use director::{SessionManager, SessionStrategy, SessionTarget};

    match cmd {
        SessionCommands::Ls { all, path } => {
            let project_path = path.canonicalize()
                .context("Failed to resolve project path")?;
            let manager = SessionManager::new(project_path);

            let sessions = if all {
                manager.list_sessions().await
            } else {
                manager.list_active().await
            };

            if sessions.is_empty() {
                if all {
                    println!("No sessions found.");
                } else {
                    println!("No active sessions. Use 'palace session create <target>' to start one.");
                }
            } else {
                println!("Sessions ({}):\n", sessions.len());
                for s in &sessions {
                    let duration = s.duration()
                        .map(|d| format!("{}s", d.num_seconds()))
                        .unwrap_or_else(|| "-".to_string());

                    let progress = if s.tasks_total > 0 {
                        format!("{}/{}", s.tasks_completed, s.tasks_total)
                    } else {
                        "-".to_string()
                    };

                    // Status with color
                    let status = match s.status {
                        director::SessionStatus::Running => format!("\x1b[32m{}\x1b[0m", s.status),
                        director::SessionStatus::Completed => format!("\x1b[34m{}\x1b[0m", s.status),
                        director::SessionStatus::Failed => format!("\x1b[31m{}\x1b[0m", s.status),
                        _ => format!("{}", s.status),
                    };

                    println!("  {} {} [{}] {} ({}) {}",
                        s.short_id(),
                        s.name,
                        status,
                        s.target,
                        s.strategy,
                        progress);

                    if let Some(task) = &s.current_task {
                        println!("      → {}", task);
                    }
                    if let Some(branch) = &s.branch {
                        println!("      \x1b[90mbranch: {}\x1b[0m", branch);
                    }
                }
            }
        }

        SessionCommands::Log { session, follow, lines, path } => {
            let project_path = path.canonicalize()
                .context("Failed to resolve project path")?;
            let manager = SessionManager::new(project_path);

            let sess = manager.find_session(&session).await
                .context(format!("Session not found: {}", session))?;

            if follow {
                // Subscribe to live events
                let mut rx = manager.subscribe();
                let session_id = sess.id;

                // Print existing logs first
                let logs = manager.get_logs(session_id, lines).await;
                for entry in &logs {
                    print_log_entry(entry);
                }

                println!("\x1b[90m--- Following session {} (Ctrl+C to stop) ---\x1b[0m", sess.short_id());

                // Follow new events
                loop {
                    match rx.recv().await {
                        Ok(director::SessionEvent::Log { session_id: id, entry }) if id == session_id => {
                            print_log_entry(&entry);
                        }
                        Ok(director::SessionEvent::StatusChanged { session_id: id, status }) if id == session_id => {
                            println!("\x1b[33m[STATUS] {}\x1b[0m", status);
                            if matches!(status, director::SessionStatus::Completed | director::SessionStatus::Failed | director::SessionStatus::Cancelled) {
                                break;
                            }
                        }
                        Ok(director::SessionEvent::Progress { session_id: id, completed, total, current }) if id == session_id => {
                            println!("\x1b[36m[{}/{}] {}\x1b[0m", completed, total, current);
                        }
                        Err(_) => break,
                        _ => {}
                    }
                }
            } else {
                let logs = manager.get_logs(sess.id, lines).await;
                if logs.is_empty() {
                    println!("No log entries for session {}.", sess.short_id());
                } else {
                    for entry in &logs {
                        print_log_entry(entry);
                    }
                }
            }
        }

        SessionCommands::Create { target, strategy, run, background, path, llm_url, model } => {
            let project_path = path.canonicalize()
                .context("Failed to resolve project path")?;
            let manager = std::sync::Arc::new(SessionManager::new(project_path.clone()));

            // Parse target
            let session_target = parse_session_target(&target)?;

            // Parse strategy
            let session_strategy: SessionStrategy = strategy.parse()
                .map_err(|e: String| anyhow::anyhow!(e))?;

            let session = manager.create_session(session_target, session_strategy).await
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            println!("Created session: {} ({})", session.name, session.short_id());
            println!("  Target: {}", session.target);
            println!("  Strategy: {}", session.strategy);
            if let Some(branch) = &session.branch {
                println!("  Branch: {}", branch);
            }
            if let Some(worktree) = &session.worktree_path {
                println!("  Worktree: {}", worktree.display());
            }

            // Run immediately if requested
            let should_run = run || background;
            if should_run {
                println!();

                let exec_config = director::SessionExecutorConfig {
                    llm_url,
                    model,
                    max_tokens: 4096,
                    workspace: "wings".to_string(),
                    project: "PAL".to_string(),
                    zulip_enabled: false,
                    zulip_stream: "palace".to_string(),
                    skills: vec![],
                };

                let mut executor = director::SessionExecutor::new(exec_config, manager.clone());

                if background {
                    // TODO: Background execution with process spawning
                    println!("\x1b[33mBackground execution not yet implemented. Running in foreground.\x1b[0m");
                }

                println!("Starting session execution...\n");

                match executor.execute(session.id).await {
                    Ok(()) => {
                        println!("\n\x1b[32mSession completed successfully!\x1b[0m");
                    }
                    Err(e) => {
                        println!("\n\x1b[31mSession failed: {}\x1b[0m", e);
                    }
                }
            } else {
                println!("\n\x1b[90mUse 'palace session run {}' to execute.\x1b[0m", session.short_id());
            }
        }

        SessionCommands::Cancel { session, path } => {
            let project_path = path.canonicalize()
                .context("Failed to resolve project path")?;
            let manager = SessionManager::new(project_path);

            let sess = manager.find_session(&session).await
                .context(format!("Session not found: {}", session))?;

            manager.cancel_session(sess.id).await
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            println!("Cancelled session: {} ({})", sess.name, sess.short_id());
        }

        SessionCommands::Info { session, path } => {
            let project_path = path.canonicalize()
                .context("Failed to resolve project path")?;
            let manager = SessionManager::new(project_path);

            let sess = manager.find_session(&session).await
                .context(format!("Session not found: {}", session))?;

            println!("Session: {} ({})", sess.name, sess.id);
            println!("  Status: {}", sess.status);
            println!("  Target: {}", sess.target);
            println!("  Strategy: {}", sess.strategy);
            println!("  Created: {}", sess.created_at);
            if let Some(started) = sess.started_at {
                println!("  Started: {}", started);
            }
            if let Some(completed) = sess.completed_at {
                println!("  Completed: {}", completed);
            }
            if sess.tasks_total > 0 {
                println!("  Progress: {}/{}", sess.tasks_completed, sess.tasks_total);
            }
            if let Some(task) = &sess.current_task {
                println!("  Current: {}", task);
            }
            if let Some(branch) = &sess.branch {
                println!("  Branch: {}", branch);
            }
            if let Some(worktree) = &sess.worktree_path {
                println!("  Worktree: {}", worktree.display());
            }
            if let Some(err) = &sess.error {
                println!("  \x1b[31mError: {}\x1b[0m", err);
            }
        }

        SessionCommands::Run { session, path, llm_url, model } => {
            let project_path = path.canonicalize()
                .context("Failed to resolve project path")?;
            let manager = std::sync::Arc::new(SessionManager::new(project_path));

            let sess = manager.find_session(&session).await
                .context(format!("Session not found: {}", session))?;

            if !sess.is_active() && sess.status != director::SessionStatus::Starting {
                anyhow::bail!("Session {} is already {}", sess.short_id(), sess.status);
            }

            println!("Running session: {} ({})", sess.name, sess.short_id());
            println!("  Target: {}", sess.target);
            println!("  Strategy: {}", sess.strategy);
            println!();

            // Create executor config
            let exec_config = director::SessionExecutorConfig {
                llm_url,
                model,
                max_tokens: 4096,
                workspace: "wings".to_string(),
                project: "PAL".to_string(),
                zulip_enabled: false,
                zulip_stream: "palace".to_string(),
                skills: vec![],
            };

            // Create and run executor
            let mut executor = director::SessionExecutor::new(exec_config, manager.clone());

            match executor.execute(sess.id).await {
                Ok(()) => {
                    println!("\n\x1b[32mSession completed successfully!\x1b[0m");
                }
                Err(e) => {
                    println!("\n\x1b[31mSession failed: {}\x1b[0m", e);
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}

fn print_log_entry(entry: &director::SessionLogEntry) {
    let level_color = match entry.level {
        director::LogLevel::Debug => "\x1b[90m",
        director::LogLevel::Info => "\x1b[0m",
        director::LogLevel::Warn => "\x1b[33m",
        director::LogLevel::Error => "\x1b[31m",
    };
    let time = entry.timestamp.format("%H:%M:%S");
    println!("{}{} [{}] {}\x1b[0m", level_color, time, format!("{:?}", entry.level).to_uppercase(), entry.message);
}

fn parse_session_target(target: &str) -> anyhow::Result<director::SessionTarget> {
    // Check for multiple comma-separated targets
    if target.contains(',') {
        let targets: Vec<director::SingleTarget> = target
            .split(',')
            .map(|t| parse_single_target(t.trim()))
            .collect::<anyhow::Result<Vec<_>>>()?;

        return Ok(director::SessionTarget::multi(targets));
    }

    // Single target
    Ok(director::SessionTarget::Single(parse_single_target(target)?))
}

fn parse_single_target(target: &str) -> anyhow::Result<director::SingleTarget> {
    // Check for explicit prefixes
    if let Some(id) = target.strip_prefix("issue:") {
        return Ok(director::SingleTarget::Issue(id.to_string()));
    }
    if let Some(id) = target.strip_prefix("module:") {
        return Ok(director::SingleTarget::Module(id.to_string()));
    }
    if let Some(id) = target.strip_prefix("cycle:") {
        return Ok(director::SingleTarget::Cycle(id.to_string()));
    }
    if let Some(id) = target.strip_prefix("goal:") {
        return Ok(director::SingleTarget::Goal(id.to_string()));
    }

    // Auto-detect based on format
    // PAL-123 format = issue
    if target.contains('-') && target.split('-').last().map(|s| s.chars().all(|c| c.is_ascii_digit())).unwrap_or(false) {
        return Ok(director::SingleTarget::Issue(target.to_string()));
    }

    // M001-xxx format = module
    if target.starts_with("M0") || target.starts_with("m0") {
        return Ok(director::SingleTarget::Module(target.to_string()));
    }

    // C0.x.x format = cycle
    if target.starts_with("C0") || target.starts_with("c0") || target.starts_with("v0") {
        return Ok(director::SingleTarget::Cycle(target.to_string()));
    }

    // Default to issue
    Ok(director::SingleTarget::Issue(target.to_string()))
}

/// Handle tool calls for Claude Code integration.
///
/// Available tools:
/// - smart_read: Token-efficient code reading with layered analysis
/// - smart_write: Structure-aware code editing
/// - plane_create_issue: Create Plane.so issue
/// - plane_list_issues: List Plane.so issues
/// - tools: List available tools
async fn handle_call(
    tool: &str,
    input: Option<&str>,
    path: &std::path::Path,
    workspace: Option<&str>,
    project: Option<&str>,
) -> anyhow::Result<()> {
    use llm_code_sdk::tools::Tool;
    use std::collections::HashMap;

    // Parse JSON input if provided
    // Note: Shell escaping can turn `!` into `\!` which is invalid JSON.
    // We unescape it here since `\!` is never valid JSON anyway.
    let input_map: HashMap<String, serde_json::Value> = if let Some(json) = input {
        let unescaped = json.replace("\\!", "!");
        serde_json::from_str(&unescaped).context("Invalid JSON input")?
    } else {
        HashMap::new()
    };

    // Resolve project path
    let project_path = path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf());

    match tool {
        "smart_read" => {
            let smart_read = llm_code_sdk::tools::smart::SmartReadTool::new(&project_path);
            let result = smart_read.call(input_map.clone()).await;

            // JECJIT: Inject related issue context if project config exists
            let jecjit_context = inject_jecjit_context(&project_path, &input_map).await;
            if !jecjit_context.is_empty() && !result.is_error() {
                let enhanced = format!("{}\n{}", result.to_content_string(), jecjit_context);
                println!("{}", enhanced);
            } else {
                print_tool_result(&result);
            }
        }

        "smart_write" => {
            let smart_write = llm_code_sdk::tools::smart::SmartWriteTool::new(&project_path);
            let result = smart_write.call(input_map.clone()).await;

            // JECJIT: Surface related issues for the file being written
            let jecjit_context = inject_jecjit_context(&project_path, &input_map).await;
            if !jecjit_context.is_empty() && !result.is_error() {
                let enhanced = format!("{}\n{}", result.to_content_string(), jecjit_context);
                println!("{}", enhanced);
            } else {
                print_tool_result(&result);
            }
        }

        "search" => {
            let search = llm_code_sdk::tools::SearchTool::new(&project_path);
            let result = search.call(input_map).await;
            print_tool_result(&result);
        }

        "plane" => {
            // Unified Plane.so API tool - systemd style
            // verb [object_type] [params] - object_type defaults to "issue"
            let verb = input_map.get("verb")
                .and_then(|v| v.as_str())
                .context("plane requires 'verb' (list, get, create, update, delete)")?;

            let object_type = input_map.get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("issue");

            let ws = input_map.get("workspace").and_then(|v| v.as_str())
                .or(workspace)
                .unwrap_or("wings");

            handle_plane(verb, object_type, ws, &input_map).await?;
        }

        "zulip" => {
            let zulip = director::ZulipTool::from_env()
                .map_err(|e| anyhow::anyhow!("Zulip not configured: {}", e))?;

            let verb = input_map.get("verb")
                .and_then(|v| v.as_str())
                .unwrap_or("send");

            match verb {
                "send" => {
                    let stream = input_map.get("stream")
                        .and_then(|v| v.as_str())
                        .unwrap_or("palace");
                    let topic = input_map.get("topic")
                        .and_then(|v| v.as_str())
                        .context("zulip send requires 'topic'")?;
                    let content = input_map.get("content")
                        .and_then(|v| v.as_str())
                        .context("zulip send requires 'content'")?;

                    let msg_id = zulip.send(stream, topic, content).await
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    println!("Sent message {} to {}/{}", msg_id, stream, topic);
                }
                "messages" | "get" => {
                    let stream = input_map.get("stream")
                        .and_then(|v| v.as_str())
                        .unwrap_or("palace");
                    let topic = input_map.get("topic")
                        .and_then(|v| v.as_str());
                    let limit = input_map.get("limit")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(10) as u32;

                    let messages = zulip.get_messages(stream, topic, limit).await
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    for msg in messages {
                        println!("---\n{}", msg.content);
                    }
                }
                "poll" => {
                    let stream = input_map.get("stream")
                        .and_then(|v| v.as_str())
                        .unwrap_or("palace");
                    let topic = input_map.get("topic")
                        .and_then(|v| v.as_str())
                        .context("zulip poll requires 'topic'")?;
                    let question = input_map.get("question")
                        .and_then(|v| v.as_str())
                        .context("zulip poll requires 'question'")?;
                    let options: Vec<&str> = input_map.get("options")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                        .unwrap_or_default();

                    let msg_id = zulip.send_poll(stream, topic, question, &options).await
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    println!("Created poll {} in {}/{}", msg_id, stream, topic);
                }
                _ => anyhow::bail!("Unknown zulip verb: '{}'. Use: send, messages, poll", verb),
            }
        }

        "tools" | "list" | "help" => {
            println!("Available Palace tools (not in Claude Code):\n");
            println!("  smart_read       Token-efficient code reading with 5-layer analysis");
            println!("                   Layers: raw, ast, call_graph, cfg, dfg, pdg");
            println!("                   Input: {{\"path\": \"...\", \"layer\": \"ast\", \"symbol\": \"...\"}}");
            println!("                   Batch: {{\"reads\": [{{\"path\": \"...\", \"layer\": \"...\"}}, ...]}}");
            println!();
            println!("  smart_write      Structure-aware code editing");
            println!("                   Operations: replace_function, replace_symbol, insert_after, delete, replace_lines");
            println!("                   Input: {{\"path\": \"...\", \"operation\": \"...\", \"target\": \"...\", \"content\": \"...\"}}");
            println!();
            println!("  search           MRS-based semantic code search");
            println!("                   Input: {{\"query\": \"...\", \"limit\": \"10\"}}");
            println!("                   Indexes codebase on demand, returns ranked results");
            println!();
            println!("  plane            Unified Plane.so API (systemd-style)");
            println!("                   Format: {{\"verb\": \"...\", \"type\": \"...\", ...}}");
            println!("                   Type defaults to 'issue' if omitted");
            println!();
            println!("                   Verbs: list, get, create, update, delete, raw");
            println!("                   Types: project, issue, cycle, module, label, state, member");
            println!();
            println!("  zulip            Zulip messaging API");
            println!("                   Verbs: send, messages, poll");
            println!();
            println!("                   Examples:");
            println!("                     {{\"verb\": \"send\", \"stream\": \"palace\", \"topic\": \"test\", \"content\": \"Hello!\"}}");
            println!("                     {{\"verb\": \"messages\", \"stream\": \"palace\", \"topic\": \"director/tealc\", \"limit\": 5}}");
            println!("                     {{\"verb\": \"poll\", \"topic\": \"votes\", \"question\": \"Yes?\", \"options\": [\"Yes\", \"No\"]}}");
            println!();
            println!("Usage: pal call <tool> --input '<json>'");
        }

        _ => {
            anyhow::bail!(
                "Unknown tool: '{}'\nRun 'pal call tools' to see available tools.",
                tool
            );
        }
    }
    Ok(())
}

fn print_tool_result(result: &llm_code_sdk::tools::ToolResult) {
    if result.is_error() {
        eprintln!("Error: {}", result.to_content_string());
        std::process::exit(1);
    } else {
        println!("{}", result.to_content_string());
    }
}

async fn inject_jecjit_context(
    project_path: &std::path::Path,
    input_map: &std::collections::HashMap<String, serde_json::Value>,
) -> String {
    let config = match palace_plane::ProjectConfig::load(project_path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let jecjit = palace_plane::JecjitContext::new(config);
    if jecjit.refresh().await.is_err() {
        return String::new();
    }

    // Load specs for gap detection
    jecjit.load_specs(project_path);

    let mut all_issues = Vec::new();

    if let Some(path) = input_map.get("path").and_then(|v| v.as_str()) {
        all_issues.extend(jecjit.context_for_file(path));
    }

    // Batch mode
    if let Some(reads) = input_map.get("reads").and_then(|v| v.as_array()) {
        for read in reads {
            if let Some(path) = read.get("path").and_then(|v| v.as_str()) {
                all_issues.extend(jecjit.context_for_file(path));
            }
        }
    }

    // Deduplicate by issue ID
    let mut seen = std::collections::HashSet::new();
    all_issues.retain(|issue| seen.insert(issue.id.clone()));

    let mut output = palace_plane::JecjitContext::format_context(&all_issues);

    // Surface spec gaps
    let gaps = jecjit.spec_gaps();
    if !gaps.is_empty() {
        output.push_str(&palace_plane::JecjitContext::format_gaps(&gaps));
    }

    output
}

fn parse_priority(s: Option<&str>) -> palace_plane::TaskPriority {
    match s {
        Some("urgent") => palace_plane::TaskPriority::Urgent,
        Some("high") => palace_plane::TaskPriority::High,
        Some("medium") => palace_plane::TaskPriority::Medium,
        Some("low") => palace_plane::TaskPriority::Low,
        _ => palace_plane::TaskPriority::None,
    }
}

/// Get existing config or auto-initialize.
async fn get_or_init_config(
    project_path: &std::path::Path,
) -> anyhow::Result<palace_plane::ProjectConfig> {
    let result = palace_plane::smart_init_project(project_path).await?;
    Ok(result.config)
}

/// Unified Plane.so API handler - systemd style.
/// verb [type] - type defaults to "issue"
async fn handle_plane(
    verb: &str,
    object_type: &str,
    workspace: &str,
    params: &std::collections::HashMap<String, serde_json::Value>,
) -> anyhow::Result<()> {
    let api_key = std::env::var("PLANE_API_KEY")
        .context("PLANE_API_KEY not set")?;
    let api_url = std::env::var("PLANE_API_URL")
        .unwrap_or_else(|_| "https://api.plane.so/api/v1".to_string());

    let client = reqwest::Client::new();

    // Shared rate limiter for Plane.so API (60 req/min limit)
    // Use rate_limiter.lock().await.acquire().await before each HTTP request
    let rate_limiter = palace_plane::get_rate_limiter();

    // Helper to get project ID (UUID) from identifier
    async fn get_project_id(
        client: &reqwest::Client,
        api_url: &str,
        api_key: &str,
        workspace: &str,
        project: &str,
    ) -> anyhow::Result<String> {
        palace_plane::rate_limit().await;
        let url = format!("{}/workspaces/{}/projects/", api_url, workspace);
        let resp: serde_json::Value = client.get(&url)
            .header("X-API-Key", api_key)
            .send().await?
            .json().await?;

        let projects = resp["results"].as_array()
            .context("Invalid projects response")?;

        for p in projects {
            if p["identifier"].as_str() == Some(project) ||
               p["id"].as_str() == Some(project) ||
               p["name"].as_str().map(|n| n.to_lowercase()) == Some(project.to_lowercase()) {
                return Ok(p["id"].as_str().unwrap().to_string());
            }
        }
        anyhow::bail!("Project '{}' not found in workspace '{}'", project, workspace)
    }

    // Helper to resolve issue ID (sequence like "51" or "PAL-51") to UUID
    async fn resolve_issue_id(
        client: &reqwest::Client,
        api_url: &str,
        api_key: &str,
        workspace: &str,
        project_id: &str,
        issue_id: &str,
    ) -> anyhow::Result<String> {
        // If it looks like a UUID already, return it
        if issue_id.len() == 36 && issue_id.contains('-') {
            return Ok(issue_id.to_string());
        }

        // Extract sequence number (handle "PAL-51" or just "51")
        let sequence: u64 = if issue_id.contains('-') {
            issue_id.split('-').last()
                .and_then(|s| s.parse().ok())
                .context("Invalid issue ID format")?
        } else {
            issue_id.parse().context("Invalid issue ID")?
        };

        // Fetch issues and find by sequence
        palace_plane::rate_limit().await;
        let url = format!("{}/workspaces/{}/projects/{}/issues/", api_url, workspace, project_id);
        let resp: serde_json::Value = client.get(&url)
            .header("X-API-Key", api_key)
            .send().await?
            .json().await?;

        let issues = resp["results"].as_array()
            .context("Invalid issues response")?;

        for i in issues {
            if i["sequence_id"].as_u64() == Some(sequence) {
                return Ok(i["id"].as_str().unwrap().to_string());
            }
        }
        anyhow::bail!("Issue {} not found", issue_id)
    }

    // Helper to resolve state name to UUID
    async fn resolve_state_id(
        client: &reqwest::Client,
        api_url: &str,
        api_key: &str,
        workspace: &str,
        project_id: &str,
        state_name: &str,
    ) -> anyhow::Result<String> {
        // If it looks like a UUID already, return it
        if state_name.len() == 36 && state_name.contains('-') {
            return Ok(state_name.to_string());
        }

        palace_plane::rate_limit().await;
        let url = format!("{}/workspaces/{}/projects/{}/states/", api_url, workspace, project_id);
        let resp: serde_json::Value = client.get(&url)
            .header("X-API-Key", api_key)
            .send().await?
            .json().await?;

        let states = resp["results"].as_array()
            .context("Invalid states response")?;

        let state_lower = state_name.to_lowercase();
        for s in states {
            let name = s["name"].as_str().unwrap_or("");
            if name.to_lowercase() == state_lower ||
               name.to_lowercase().contains(&state_lower) {
                return Ok(s["id"].as_str().unwrap().to_string());
            }
        }
        anyhow::bail!("State '{}' not found", state_name)
    }

    match (verb, object_type) {
        // === PROJECTS ===
        ("list", "project" | "projects") => {
            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/", api_url, workspace);
            let resp: serde_json::Value = client.get(&url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;

            if let Some(projects) = resp["results"].as_array() {
                for p in projects {
                    println!("{}: {} ({})",
                        p["identifier"].as_str().unwrap_or("?"),
                        p["name"].as_str().unwrap_or("?"),
                        p["id"].as_str().unwrap_or("?"));
                }
            }
        }

        ("create", "project" | "projects") => {
            let name = params.get("name").and_then(|v| v.as_str())
                .context("name required")?;
            let identifier = params.get("identifier").and_then(|v| v.as_str())
                .context("identifier required (e.g., AST)")?;

            let mut body = serde_json::json!({
                "name": name,
                "identifier": identifier,
                "network": 2  // 2 = private (default)
            });

            // Optional description
            if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
                body["description"] = serde_json::json!(desc);
            }

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/", api_url, workspace);
            let resp: serde_json::Value = client.post(&url)
                .header("X-API-Key", &api_key)
                .json(&body)
                .send().await?.json().await?;

            println!("Created project: {} ({}) -> {}",
                resp["identifier"].as_str().unwrap_or("?"),
                resp["name"].as_str().unwrap_or("?"),
                resp["id"].as_str().unwrap_or("?"));
        }

        // === ISSUES ===
        ("list", "issue" | "issues") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required for listing issues")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            // First get states for this project
            rate_limiter.lock().await.acquire().await;
            let states_url = format!("{}/workspaces/{}/projects/{}/states/",
                api_url, workspace, project_id);
            let states_resp: serde_json::Value = client.get(&states_url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;
            let states: std::collections::HashMap<String, String> = states_resp["results"]
                .as_array()
                .map(|arr| arr.iter()
                    .filter_map(|s| Some((s["id"].as_str()?.to_string(), s["name"].as_str()?.to_string())))
                    .collect())
                .unwrap_or_default();

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/issues/",
                api_url, workspace, project_id);
            let resp: serde_json::Value = client.get(&url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;

            if let Some(issues) = resp["results"].as_array() {
                for i in issues {
                    let priority = i["priority"].as_str().unwrap_or("none");
                    let state_id = i["state"].as_str().unwrap_or("");
                    let state_name = states.get(state_id).map(|s| s.as_str()).unwrap_or("?");
                    println!("{}-{}: {} [{}] ({})",
                        project.to_uppercase(),
                        i["sequence_id"].as_u64().unwrap_or(0),
                        i["name"].as_str().unwrap_or("?"),
                        state_name,
                        priority);
                }
            }
        }

        ("get", "issue" | "issues") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let issue_id = params.get("id").and_then(|v| v.as_str())
                .context("id required (issue UUID or sequence like PAL-123)")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/issues/{}/",
                api_url, workspace, project_id, issue_id);
            let resp: serde_json::Value = client.get(&url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;

            println!("{}", serde_json::to_string_pretty(&resp)?);
        }

        ("create", "issue" | "issues") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let name = params.get("name").and_then(|v| v.as_str())
                .context("name required")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            let mut body = serde_json::json!({
                "name": name,
                "project_id": project_id
            });

            // Optional fields
            if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
                body["description_html"] = serde_json::json!(format!("<p>{}</p>", desc));
            }
            if let Some(p) = params.get("priority").and_then(|v| v.as_str()) {
                body["priority"] = serde_json::json!(p);
            }
            if let Some(state) = params.get("state").and_then(|v| v.as_str()) {
                body["state"] = serde_json::json!(state);
            }

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/issues/",
                api_url, workspace, project_id);
            let resp: serde_json::Value = client.post(&url)
                .header("X-API-Key", &api_key)
                .json(&body)
                .send().await?.json().await?;

            println!("{}-{}: {}",
                project.to_uppercase(),
                resp["sequence_id"].as_u64().unwrap_or(0),
                resp["name"].as_str().unwrap_or("?"));
        }

        ("update", "issue" | "issues") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let issue_id_input = params.get("id").and_then(|v| v.as_str())
                .context("id required (e.g., PAL-51 or 51)")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            // Resolve sequence ID to UUID if needed
            let issue_uuid = resolve_issue_id(&client, &api_url, &api_key, workspace, &project_id, issue_id_input).await?;

            // Extract cycle if present - handled via separate endpoint
            let cycle_id = params.get("cycle").and_then(|v| v.as_str()).map(|s| s.to_string());

            let mut body = serde_json::Map::new();
            for (k, v) in params {
                // Skip meta fields and cycle (handled separately)
                if ["project", "id", "workspace", "verb", "type", "cycle"].contains(&k.as_str()) {
                    continue;
                }
                // Handle state specially - resolve state name to UUID
                if k == "state" {
                    if let Some(state_name) = v.as_str() {
                        if let Ok(state_id) = resolve_state_id(&client, &api_url, &api_key, workspace, &project_id, state_name).await {
                            body.insert(k.clone(), serde_json::json!(state_id));
                            continue;
                        }
                    }
                }
                body.insert(k.clone(), v.clone());
            }

            // Update issue fields if any (besides cycle)
            if !body.is_empty() {
                rate_limiter.lock().await.acquire().await;
                let url = format!("{}/workspaces/{}/projects/{}/issues/{}/",
                    api_url, workspace, project_id, issue_uuid);
                let resp = client.patch(&url)
                    .header("X-API-Key", &api_key)
                    .json(&body)
                    .send().await?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("Failed to update issue: {} - {}", status, text);
                }
            } else if cycle_id.is_none() {
                // No fields to update and no cycle - this is a no-op, warn
                eprintln!("Warning: No fields to update");
            }

            // Add to cycle via separate endpoint if cycle specified
            if let Some(cycle) = cycle_id {
                rate_limiter.lock().await.acquire().await;
                let cycle_url = format!("{}/workspaces/{}/projects/{}/cycles/{}/cycle-issues/",
                    api_url, workspace, project_id, cycle);
                let cycle_body = serde_json::json!({
                    "issues": [issue_uuid]
                });
                let cycle_resp = client.post(&cycle_url)
                    .header("X-API-Key", &api_key)
                    .json(&cycle_body)
                    .send().await?;

                if !cycle_resp.status().is_success() {
                    let status = cycle_resp.status();
                    let text = cycle_resp.text().await.unwrap_or_default();
                    eprintln!("Warning: Failed to add to cycle: {} - {}", status, text);
                }
            }

            // Normalize issue ID for display (strip project prefix if present)
            let display_id = if issue_id_input.to_uppercase().starts_with(&format!("{}-", project.to_uppercase())) {
                &issue_id_input[project.len() + 1..]
            } else {
                issue_id_input
            };
            println!("Updated: {}-{}", project.to_uppercase(), display_id);
        }

        ("delete", "issue" | "issues") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let issue_id = params.get("id").and_then(|v| v.as_str())
                .context("id required")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/issues/{}/",
                api_url, workspace, project_id, issue_id);
            client.delete(&url)
                .header("X-API-Key", &api_key)
                .send().await?;

            println!("Deleted: {}", issue_id);
        }

        // === CYCLES ===
        ("list", "cycle" | "cycles") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/cycles/",
                api_url, workspace, project_id);
            let resp: serde_json::Value = client.get(&url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;

            if let Some(cycles) = resp["results"].as_array() {
                for c in cycles {
                    println!("{}: {} ({} - {})",
                        c["id"].as_str().unwrap_or("?"),
                        c["name"].as_str().unwrap_or("?"),
                        c["start_date"].as_str().unwrap_or("?"),
                        c["end_date"].as_str().unwrap_or("?"));
                }
            }
        }

        ("create", "cycle" | "cycles") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let name = params.get("name").and_then(|v| v.as_str())
                .context("name required")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            let mut body = serde_json::json!({
                "name": name,
                "project_id": project_id
            });
            if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
                body["description"] = serde_json::json!(desc);
            }
            if let Some(start) = params.get("start_date").and_then(|v| v.as_str()) {
                body["start_date"] = serde_json::json!(start);
            }
            if let Some(end) = params.get("end_date").and_then(|v| v.as_str()) {
                body["end_date"] = serde_json::json!(end);
            }

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/cycles/",
                api_url, workspace, project_id);
            let resp = client.post(&url)
                .header("X-API-Key", &api_key)
                .json(&body)
                .send().await?;

            let status = resp.status();
            let resp_json: serde_json::Value = resp.json().await?;

            if !status.is_success() {
                anyhow::bail!("Failed to create cycle: {}", resp_json);
            }

            println!("Created cycle: {} ({})",
                resp_json["name"].as_str().unwrap_or("?"),
                resp_json["id"].as_str().unwrap_or("?"));
        }

        ("update", "cycle" | "cycles") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let cycle_id = params.get("id").and_then(|v| v.as_str())
                .context("id required (cycle UUID)")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            let mut body = serde_json::Map::new();
            for (k, v) in params {
                if !["project", "id", "workspace", "verb", "type"].contains(&k.as_str()) {
                    body.insert(k.clone(), v.clone());
                }
            }

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/cycles/{}/",
                api_url, workspace, project_id, cycle_id);
            let resp: serde_json::Value = client.patch(&url)
                .header("X-API-Key", &api_key)
                .json(&body)
                .send().await?.json().await?;

            println!("Updated cycle: {}", resp["name"].as_str().unwrap_or("?"));
        }

        ("delete", "cycle" | "cycles") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let cycle_id = params.get("id").and_then(|v| v.as_str())
                .context("id required (cycle UUID)")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/cycles/{}/",
                api_url, workspace, project_id, cycle_id);
            client.delete(&url)
                .header("X-API-Key", &api_key)
                .send().await?;

            println!("Deleted cycle: {}", cycle_id);
        }

        // === MODULES ===
        ("list", "module" | "modules") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/modules/",
                api_url, workspace, project_id);
            let resp: serde_json::Value = client.get(&url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;

            if let Some(modules) = resp["results"].as_array() {
                for m in modules {
                    println!("{}: {}",
                        m["id"].as_str().unwrap_or("?"),
                        m["name"].as_str().unwrap_or("?"));
                }
            }
        }

        ("create", "module" | "modules") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let name = params.get("name").and_then(|v| v.as_str())
                .context("name required")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            let body = serde_json::json!({"name": name});

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/modules/",
                api_url, workspace, project_id);
            let resp: serde_json::Value = client.post(&url)
                .header("X-API-Key", &api_key)
                .json(&body)
                .send().await?.json().await?;

            println!("Created module: {}", resp["name"].as_str().unwrap_or("?"));
        }

        // === LABELS ===
        ("list", "label" | "labels") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/labels/",
                api_url, workspace, project_id);
            let resp: serde_json::Value = client.get(&url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;

            if let Some(labels) = resp["results"].as_array() {
                for l in labels {
                    println!("{}: {} ({})",
                        l["id"].as_str().unwrap_or("?"),
                        l["name"].as_str().unwrap_or("?"),
                        l["color"].as_str().unwrap_or("?"));
                }
            }
        }

        // === STATES ===
        ("list", "state" | "states") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            rate_limiter.lock().await.acquire().await;
            let url = format!("{}/workspaces/{}/projects/{}/states/",
                api_url, workspace, project_id);
            let resp: serde_json::Value = client.get(&url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;

            if let Some(states) = resp["results"].as_array() {
                for s in states {
                    println!("{}: {} [{}]",
                        s["id"].as_str().unwrap_or("?"),
                        s["name"].as_str().unwrap_or("?"),
                        s["group"].as_str().unwrap_or("?"));
                }
            }
        }

        // === MEMBERS ===
        ("list", "member" | "members") => {
            let project = params.get("project").and_then(|v| v.as_str());

            let url = if let Some(proj) = project {
                let project_id = get_project_id(&client, &api_url, &api_key, workspace, proj).await?;
                format!("{}/workspaces/{}/projects/{}/members/",
                    api_url, workspace, project_id)
            } else {
                format!("{}/workspaces/{}/members/", api_url, workspace)
            };

            rate_limiter.lock().await.acquire().await;
            let resp: serde_json::Value = client.get(&url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;

            // Handle both array and paginated response
            let members = resp.as_array()
                .or_else(|| resp["results"].as_array());

            if let Some(members) = members {
                for m in members {
                    let member = &m["member"];
                    println!("{}: {} <{}>",
                        member["id"].as_str().unwrap_or("?"),
                        member["display_name"].as_str()
                            .or(member["first_name"].as_str())
                            .unwrap_or("?"),
                        member["email"].as_str().unwrap_or("?"));
                }
            }
        }

        // === BATCH UPDATE ===
        ("batch", "issue" | "issues") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            // Get updates array: [{"id": "...", "state": "...", ...}, ...]
            let updates = params.get("updates").and_then(|v| v.as_array())
                .context("updates array required")?;

            for update in updates {
                let issue_id = update.get("id").and_then(|v| v.as_str())
                    .context("each update needs 'id'")?;

                // Resolve sequence ID (like "39") to UUID if needed
                let resolved_id = if issue_id.chars().all(|c| c.is_ascii_digit()) {
                    // It's a sequence number, look it up
                    let seq: u64 = issue_id.parse()?;
                    rate_limiter.lock().await.acquire().await;
                    let list_url = format!("{}/workspaces/{}/projects/{}/issues/",
                        api_url, workspace, project_id);
                    let list_resp: serde_json::Value = client.get(&list_url)
                        .header("X-API-Key", &api_key)
                        .send().await?.json().await?;

                    let issues = list_resp["results"].as_array()
                        .context("Invalid response")?;
                    let found = issues.iter()
                        .find(|i| i["sequence_id"].as_u64() == Some(seq))
                        .and_then(|i| i["id"].as_str())
                        .context(format!("Issue {} not found", issue_id))?;
                    found.to_string()
                } else {
                    issue_id.to_string()
                };

                let mut body = serde_json::Map::new();
                for (k, v) in update.as_object().unwrap() {
                    if k != "id" {
                        body.insert(k.clone(), v.clone());
                    }
                }

                rate_limiter.lock().await.acquire().await;
                let url = format!("{}/workspaces/{}/projects/{}/issues/{}/",
                    api_url, workspace, project_id, resolved_id);
                let _resp: serde_json::Value = client.patch(&url)
                    .header("X-API-Key", &api_key)
                    .json(&body)
                    .send().await?.json().await?;

                println!("Updated: {}-{}", project.to_uppercase(), issue_id);
            }
        }

        // === QUERY ISSUES ===
        ("query", "issue" | "issues") => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            // First get states for this project
            let states_url = format!("{}/workspaces/{}/projects/{}/states/",
                api_url, workspace, project_id);
            let states_resp: serde_json::Value = client.get(&states_url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;
            let states: std::collections::HashMap<String, String> = states_resp["results"]
                .as_array()
                .map(|arr| arr.iter()
                    .filter_map(|s| Some((s["id"].as_str()?.to_string(), s["name"].as_str()?.to_string())))
                    .collect())
                .unwrap_or_default();

            let url = format!("{}/workspaces/{}/projects/{}/issues/",
                api_url, workspace, project_id);
            let resp: serde_json::Value = client.get(&url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;

            let issues = resp["results"].as_array().context("Invalid response")?;

            // Apply filters
            let state_filter = params.get("state").and_then(|v| v.as_str());
            let priority_filter = params.get("priority").and_then(|v| v.as_str());
            let search = params.get("search").and_then(|v| v.as_str());

            for i in issues {
                let state_id = i["state"].as_str().unwrap_or("");
                let state_name = states.get(state_id).map(|s| s.as_str()).unwrap_or("?");
                let priority = i["priority"].as_str().unwrap_or("none");
                let name = i["name"].as_str().unwrap_or("?");
                let seq = i["sequence_id"].as_u64().unwrap_or(0);
                let uuid = i["id"].as_str().unwrap_or("?");

                // Apply filters
                if let Some(sf) = state_filter {
                    if !state_name.to_lowercase().contains(&sf.to_lowercase()) &&
                       !state_id.contains(sf) {
                        continue;
                    }
                }
                if let Some(pf) = priority_filter {
                    if priority != pf {
                        continue;
                    }
                }
                if let Some(s) = search {
                    if !name.to_lowercase().contains(&s.to_lowercase()) {
                        continue;
                    }
                }

                println!("{}-{}: {} [{}] ({}) -> {}",
                    project.to_uppercase(), seq, name, state_name, priority, uuid);
            }
        }

        // === RAW API CALL ===
        ("raw", _) => {
            let method = params.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
            let path = params.get("path").and_then(|v| v.as_str())
                .context("path required for raw API call")?;
            let body = params.get("body");

            let url = format!("{}{}", api_url, path);

            let mut req = match method.to_uppercase().as_str() {
                "GET" => client.get(&url),
                "POST" => client.post(&url),
                "PATCH" => client.patch(&url),
                "PUT" => client.put(&url),
                "DELETE" => client.delete(&url),
                _ => anyhow::bail!("Unknown method: {}", method),
            };

            req = req.header("X-API-Key", &api_key);

            if let Some(b) = body {
                req = req.json(b);
            }

            let resp: serde_json::Value = req.send().await?.json().await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }

        // === COMPARE ===
        ("compare", _) => {
            let project = params.get("project").and_then(|v| v.as_str())
                .context("project required")?;
            let files: Vec<String> = params.get("files")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();

            if files.is_empty() {
                anyhow::bail!("files required (array of spec file paths to compare against)");
            }

            let project_id = get_project_id(&client, &api_url, &api_key, workspace, project).await?;

            // Fetch current issues
            let url = format!("{}/workspaces/{}/projects/{}/issues/", api_url, workspace, project_id);
            let resp: serde_json::Value = client.get(&url)
                .header("X-API-Key", &api_key)
                .send().await?.json().await?;

            let issues: Vec<palace_plane::api::PlaneIssue> = resp["results"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|i| {
                    Some(palace_plane::api::PlaneIssue {
                        id: i["id"].as_str()?.to_string(),
                        sequence_id: i["sequence_id"].as_u64()? as u32,
                        name: i["name"].as_str()?.to_string(),
                        description_html: i["description_html"].as_str().map(|s| s.to_string()),
                        state: i["state"].as_str().map(|s| s.to_string()),
                        priority: i["priority"].as_str().map(|s| s.to_string()),
                    })
                }).collect())
                .unwrap_or_default();

            let lm_url = std::env::var("LM_STUDIO_URL")
                .unwrap_or_else(|_| "http://localhost:1234/v1".to_string());

            let request = palace_plane::comparison::CompareRequest {
                spec_files: files,
                check_code: params.get("check_code").and_then(|v| v.as_bool()).unwrap_or(false),
                check_plane: true,
                project_path: params.get("project_path").and_then(|v| v.as_str()).map(|s| s.to_string()),
            };

            let result = palace_plane::comparison::compare(request, &issues, &lm_url).await?;

            println!("{}", result.summary);
            if !result.gaps.is_empty() {
                println!("\n## Gaps ({}):", result.gaps.len());
                for gap in &result.gaps {
                    println!("- {} [{}]", gap.description, format!("{:?}", gap.gap_type));
                    if let Some(action) = &gap.suggested_action {
                        println!("  → {}", action);
                    }
                }
            }
            if !result.matches.is_empty() {
                println!("\n## Matches ({}):", result.matches.len());
                for m in result.matches.iter().take(10) {
                    println!("- {} ✓ ({})", m.description, m.evidence);
                }
            }
        }

        _ => {
            anyhow::bail!("Unknown: {} {}\nVerbs: list, get, create, update, delete, compare, raw\nTypes: project, issue, cycle, module, label, state, member", verb, object_type);
        }
    }

    Ok(())
}

/// Format a suggestion as Zulip markdown.
fn format_suggestion_markdown(index: usize, task: &palace_plane::PendingTask) -> String {
    let mut content = format!("## #{} {}\n\n", index, task.title);

    if let Some(desc) = &task.description {
        content.push_str(desc);
        content.push_str("\n\n");
    }

    if let Some(plan) = &task.plan {
        content.push_str("**Plan:**\n");
        for (i, step) in plan.iter().enumerate() {
            content.push_str(&format!("{}. {}\n", i + 1, step));
        }
        content.push('\n');
    }

    if let Some(subtasks) = &task.subtasks {
        if !subtasks.is_empty() {
            content.push_str("**Subtasks:**\n");
            for st in subtasks {
                content.push_str(&format!("- {}\n", st));
            }
            content.push('\n');
        }
    }

    if let Some(relations) = &task.relations {
        if !relations.is_empty() {
            content.push_str("**Relations:**\n");
            for rel in relations {
                let rel_type = match rel.relation_type {
                    palace_plane::RelationType::DependsOn => "depends on",
                    palace_plane::RelationType::Blocks => "blocks",
                    palace_plane::RelationType::RelatedTo => "related to",
                };
                let reason = rel.reason.as_deref()
                    .map(|r| format!(" *({})*", r))
                    .unwrap_or_default();
                content.push_str(&format!("- {} #{}{}\n", rel_type, rel.target_index, reason));
            }
            content.push('\n');
        }
    }

    // Add effort/priority if present
    let mut meta = Vec::new();
    if let Some(effort) = &task.effort {
        meta.push(format!("Effort: {}", effort));
    }
    if task.priority != palace_plane::TaskPriority::None {
        meta.push(format!("Priority: {}", task.priority.as_str()));
    }
    if !meta.is_empty() {
        content.push_str(&format!("*{}*\n", meta.join(" | ")));
    }

    content
}

/// Format all suggestions as ONE merged markdown message.
fn format_merged_suggestions(tasks: &[(usize, palace_plane::PendingTask)]) -> String {
    let mut content = String::from("# 🏛️ Suggestions\n\n");

    for (index, task) in tasks {
        // Compact format: ### #N Title
        content.push_str(&format!("### #{} {}\n", index, task.title));

        if let Some(desc) = &task.description {
            content.push_str(desc);
            content.push_str("\n\n");
        }

        // Plan as numbered list
        if let Some(plan) = &task.plan {
            for (i, step) in plan.iter().enumerate() {
                content.push_str(&format!("{}. {}\n", i + 1, step));
            }
            content.push('\n');
        }

        // Relations inline
        if let Some(relations) = &task.relations {
            if !relations.is_empty() {
                let rel_strs: Vec<String> = relations.iter().map(|rel| {
                    let rel_type = match rel.relation_type {
                        palace_plane::RelationType::DependsOn => "→",
                        palace_plane::RelationType::Blocks => "⊢",
                        palace_plane::RelationType::RelatedTo => "~",
                    };
                    format!("{} #{}", rel_type, rel.target_index)
                }).collect();
                content.push_str(&format!("*{}*\n\n", rel_strs.join(", ")));
            }
        }

        content.push_str("---\n\n");
    }

    content.push_str(&format!(
        "**{}** suggestions • `@palace approve 1,2,3` to create issues",
        tasks.len()
    ));

    content
}

/// Format suggestions as a Zulip poll for voting.
fn format_suggestions_poll(tasks: &[(usize, palace_plane::PendingTask)]) -> String {
    let mut poll = String::from("/poll What should we work on?\n");

    for (index, task) in tasks {
        // Short title, truncated if needed
        let title = if task.title.len() > 50 {
            format!("{}...", &task.title[..47])
        } else {
            task.title.clone()
        };
        poll.push_str(&format!("{}: {}\n", index, title));
    }

    poll
}

/// Parse comma-separated numbers.
fn parse_numbers(s: &str) -> anyhow::Result<Vec<usize>> {
    s.split(',')
        .map(|n| n.trim().parse::<usize>())
        .collect::<Result<Vec<_>, _>>()
        .context("Invalid number format. Use comma-separated numbers like: 1,2,3")
}
