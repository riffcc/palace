//! Run the Palace bot.
//!
//! Run with: cargo run -p palace-zulip --example bot_test
//!
//! Requires .env file with:
//! - ZULIP_SERVER_URL
//! - ZULIP_BOT_EMAIL
//! - ZULIP_API_KEY
//! - ZULIP_INSECURE (optional, for self-signed certs)

use palace_zulip::{PalaceBot, PalaceBotConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // Load from .env
    dotenvy::dotenv().ok();

    let config = PalaceBotConfig::from_env()?;
    println!("Starting Palace bot '{}'", config.name);
    println!("Server: {}", config.server_url);

    let bot = PalaceBot::new(config)?;

    println!("Connecting...");
    bot.connect().await?;
    println!("Connected!");

    println!("Announcing startup...");
    bot.announce_startup().await?;

    println!("Listening for messages... (Ctrl+C to stop)");
    println!("Mention me with: @**Palace** help");

    bot.run().await?;

    Ok(())
}
