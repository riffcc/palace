//! Example: Using ZulipTool from Claude Code
//!
//! This demonstrates how to use the ZulipTool to send messages to Zulip,
//! enabling real-time communication during development sessions.
//!
//! Usage:
//!   cargo run -p director --example zulip_tool -- <command> [args...]
//!
//! Commands:
//!   send <stream> <topic> <message>  - Send a message
//!   palace <command>                  - Send a command to Palace bot
//!   messages <stream> [topic]         - Get recent messages
//!   subscribe <stream>                - Subscribe to a stream
//!
//! Examples:
//!   cargo run -p director --example zulip_tool -- send general greetings "Hello from Claude Code!"
//!   cargo run -p director --example zulip_tool -- palace "status"
//!   cargo run -p director --example zulip_tool -- messages general commands

use director::{ZulipTool, ZulipReporter, SurveyOption};
use std::env;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let command = &args[1];

    match command.as_str() {
        "send" => {
            if args.len() < 5 {
                eprintln!("Usage: zulip_tool send <stream> <topic> <message>");
                return Ok(());
            }
            let stream = &args[2];
            let topic = &args[3];
            let message = args[4..].join(" ");

            let tool = ZulipTool::from_env()?;
            let msg_id = tool.send(stream, topic, &message).await?;
            println!("✅ Message sent! ID: {}", msg_id);
        }

        "palace" => {
            if args.len() < 3 {
                eprintln!("Usage: zulip_tool palace <command>");
                return Ok(());
            }
            let cmd = args[2..].join(" ");

            let tool = ZulipTool::from_env()?;
            let msg_id = tool.palace(&cmd).await?;
            println!("✅ Palace command sent! ID: {}", msg_id);
            println!("   Command: @**Palace** {}", cmd);
        }

        "messages" => {
            if args.len() < 3 {
                eprintln!("Usage: zulip_tool messages <stream> [topic]");
                return Ok(());
            }
            let stream = &args[2];
            let topic = if args.len() > 3 { Some(args[3].as_str()) } else { None };

            let tool = ZulipTool::from_env()?;
            let messages = tool.get_messages(stream, topic, 10).await?;

            println!("📬 Recent messages from {}{}:", stream, topic.map(|t| format!("/{}", t)).unwrap_or_default());
            for msg in messages {
                println!("\n[{}] {}:", msg.sender_full_name, msg.subject);
                println!("  {}", msg.text().lines().take(3).collect::<Vec<_>>().join("\n  "));
            }
        }

        "subscribe" => {
            if args.len() < 3 {
                eprintln!("Usage: zulip_tool subscribe <stream>");
                return Ok(());
            }
            let stream = &args[2];

            let tool = ZulipTool::from_env()?;
            tool.subscribe(stream).await?;
            println!("✅ Subscribed to stream: {}", stream);
        }

        "report" => {
            // Demo: Send a full session report
            let mut reporter = ZulipReporter::from_env()?;
            let session_id = Uuid::new_v4();
            let session_name = "demo-session";

            println!("📊 Sending demo session reports...");

            // Session started
            reporter.session_started(session_id, session_name, "PAL-52").await?;
            println!("  ✓ Session started");

            // Progress update
            reporter.session_progress(session_id, session_name, 1, 3, "Building Zulip integration").await?;
            println!("  ✓ Progress update");

            // Tool call
            reporter.tool_call(session_id, session_name, "read_file", "Reading src/lib.rs").await?;
            println!("  ✓ Tool call");

            println!("\n✅ Demo reports sent to palace stream!");
            println!("   Check Zulip topic: session/{}: {}", &session_id.to_string()[..8], session_name);
        }

        "survey" => {
            // Demo: Send a survey
            let mut reporter = ZulipReporter::from_env()?;
            let session_id = Uuid::new_v4();
            let session_name = "demo-survey";

            let options = vec![
                SurveyOption::new("👍", "Looks good", "approve"),
                SurveyOption::new("👎", "Needs changes", "reject"),
                SurveyOption::new("❓", "Need more info", "info"),
            ];

            let msg_id = reporter.survey(
                session_id,
                session_name,
                "How does the Zulip integration look so far?",
                &options,
            ).await?;

            println!("📋 Survey sent! Message ID: {}", msg_id);
            println!("   React with emoji to respond");
        }

        _ => {
            eprintln!("Unknown command: {}", command);
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!("ZulipTool - Palace Zulip Integration

Usage: zulip_tool <command> [args...]

Commands:
  send <stream> <topic> <message>  Send a message to Zulip
  palace <command>                 Send a command to Palace bot
  messages <stream> [topic]        Get recent messages
  subscribe <stream>               Subscribe to a stream
  report                           Demo: Send session reports
  survey                           Demo: Send a survey

Environment Variables:
  ZULIP_SERVER_URL    Zulip server URL (default: https://localhost:8443)
  DIRECTOR_BOT_EMAIL  Bot email address
  DIRECTOR_API_KEY    Bot API key
  ZULIP_INSECURE      Set to 'true' for self-signed certs

Examples:
  # Send a message
  cargo run -p director --example zulip_tool -- send general greetings 'Hello!'

  # Command Palace bot
  cargo run -p director --example zulip_tool -- palace 'status'

  # Get recent messages
  cargo run -p director --example zulip_tool -- messages palace

  # Demo session reporting
  cargo run -p director --example zulip_tool -- report
");
}
