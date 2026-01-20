//! Example: Run the Palace Daemon in event-driven mode.
//!
//! The daemon subscribes to Zulip events and responds to messages/reactions.
//! Mention @Director in Zulip to interact.
//!
//! Usage:
//!   cargo run -p director --example daemon
//!   cargo run -p director --example daemon -- lm:/nvidia_orchestrator-8b@q6_k_l
//!   cargo run -p director --example daemon -- orch
//!
//! Model prefixes:
//!   lm:/ = Local LM Studio (default)
//!   z:/  = Z.ai API
//!   or:/ = OpenRouter
//!   ms:/ = Mistral API
//!
//! Model aliases:
//!   flash     = lm:/glm-4.7-flash (default)
//!   orch      = lm:/nvidia_orchestrator-8b@q6_k_l
//!   devstral  = lm:/devstral-small-2-24b-instruct-2512
//!   glm       = z:/glm-4.7
//!   gpt       = or:/openai/gpt-5.2
//!   mistral   = ms:/devstral-2
//!
//! Environment:
//!   ZULIP_SERVER_URL, DIRECTOR_BOT_EMAIL, DIRECTOR_API_KEY, ZULIP_INSECURE

use director::{Daemon, DaemonConfig, ModelRegistry, parse_model};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("director=info".parse()?)
                .add_directive("hyper=warn".parse()?)
        )
        .init();

    // Load environment
    let _ = dotenvy::from_path(std::path::Path::new(&std::env::var("HOME")?).join("ai/zulip/.env"));

    // Get model from CLI arg or env var
    let registry = ModelRegistry::standard();
    let model_arg = std::env::args().nth(1)
        .or_else(|| std::env::var("DIRECTOR_MODEL").ok())
        .unwrap_or_else(|| "flash".to_string());
    let model = registry.resolve(&model_arg);
    let (route, model_name) = parse_model(&model);

    let config = DaemonConfig {
        project_path: PathBuf::from("."),
        model: model.clone(),
        zulip_enabled: true,
        zulip_stream: "palace".to_string(),
        web_addr: "127.0.0.1:3456".parse()?,
        ..Default::default()
    };

    println!("🏰 Palace Daemon starting...");
    println!("   Model: {} via {:?}", model_name, route);
    println!("   Zulip: event-driven mode enabled");
    println!("   Web UI: http://{}", config.web_addr);
    println!();
    println!("Mention @Director in Zulip to interact!");
    println!();

    let daemon = Daemon::new(config)?;
    daemon.run().await?;

    Ok(())
}
