//! DirectorControl tool for Palace.
//!
//! Allows Palace to interact with running Director pools via `pal call DirectorControl`.
//!
//! Commands:
//!   status                    - Show all Directors
//!   list                      - List Director names
//!   add <name> [model]        - Add a Director
//!   goal <name> <description> - Set a goal
//!   issue <name> <id>         - Assign an issue
//!   model <name> <model>      - Change model
//!   say <name> <message>      - Send to Zulip
//!   telepathy <from> <to> <message> - Send telepathy message

use crate::{DirectorResult, DirectorError};
use crate::pool::{Pool, TelepathyKind, PoolStatus};
use llm_code_sdk::Tool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

/// DirectorControl tool for Palace integration.
pub struct DirectorControlTool {
    pool: Arc<RwLock<Pool>>,
}

impl DirectorControlTool {
    pub fn new(pool: Arc<RwLock<Pool>>) -> Self {
        Self { pool }
    }

    /// Execute a command.
    pub async fn execute(&self, command: &str, args: &[&str]) -> DirectorResult<String> {
        match command {
            "status" => {
                let pool = self.pool.read().await;
                let status = pool.status().await;
                Ok(serde_json::to_string_pretty(&status)?)
            }

            "list" => {
                let pool = self.pool.read().await;
                let names = pool.list();
                Ok(names.join("\n"))
            }

            "add" => {
                if args.is_empty() {
                    return Err(DirectorError::Config("Usage: add <name> [model]".into()));
                }
                let name = args[0];
                let model = args.get(1).copied().unwrap_or("flash");
                let mut pool = self.pool.write().await;
                pool.add(name, model)?;
                Ok(format!("Added Director '{}' with model '{}'", name, model))
            }

            "goal" => {
                if args.len() < 2 {
                    return Err(DirectorError::Config("Usage: goal <name> <description>".into()));
                }
                let name = args[0];
                let description = args[1..].join(" ");
                let pool = self.pool.read().await;

                if let Some(director) = pool.get(name) {
                    // Send goal via telepathy (internal) and Zulip (external)
                    let dir = director.read().await;
                    dir.send(None, TelepathyKind::Status, &format!("Goal set: {}", description)).await;
                    drop(dir);

                    pool.zulip_send("palace", &format!("director/{}", name),
                        &format!("🎯 **Goal Set**\n\n{}", description)).await?;

                    Ok(format!("Goal set for '{}': {}", name, description))
                } else {
                    Err(DirectorError::Config(format!("Director '{}' not found", name)))
                }
            }

            "issue" => {
                if args.len() < 2 {
                    return Err(DirectorError::Config("Usage: issue <name> <id>".into()));
                }
                let name = args[0];
                let issue_id = args[1];
                let pool = self.pool.read().await;

                if let Some(director) = pool.get(name) {
                    let dir = director.read().await;
                    dir.send(None, TelepathyKind::Claim, &format!("issue:{}", issue_id)).await;
                    drop(dir);

                    pool.zulip_send("palace", &format!("director/{}", name),
                        &format!("📋 Working on issue `{}`", issue_id)).await?;

                    Ok(format!("Assigned issue '{}' to '{}'", issue_id, name))
                } else {
                    Err(DirectorError::Config(format!("Director '{}' not found", name)))
                }
            }

            "model" => {
                if args.len() < 2 {
                    return Err(DirectorError::Config("Usage: model <name> <model>".into()));
                }
                let name = args[0];
                let model = args[1];
                let pool = self.pool.read().await;

                if let Some(director) = pool.get(name) {
                    let mut dir = director.write().await;
                    let old_model = dir.model.clone();
                    dir.model = pool.registry.resolve(model);
                    let new_model = dir.model.clone();
                    drop(dir);

                    pool.zulip_send("palace", &format!("director/{}", name),
                        &format!("🔄 Model changed: `{}` → `{}`", old_model, new_model)).await?;

                    Ok(format!("Changed '{}' model to '{}'", name, new_model))
                } else {
                    Err(DirectorError::Config(format!("Director '{}' not found", name)))
                }
            }

            "say" => {
                if args.len() < 2 {
                    return Err(DirectorError::Config("Usage: say <name> <message>".into()));
                }
                let name = args[0];
                let message = args[1..].join(" ");
                let pool = self.pool.read().await;

                pool.zulip_send("palace", &format!("director/{}", name), &message).await?;
                Ok(format!("Sent to Zulip: {}", message))
            }

            "telepathy" => {
                if args.len() < 3 {
                    return Err(DirectorError::Config("Usage: telepathy <from> <to> <message>".into()));
                }
                let from = args[0];
                let to = args[1];
                let message = args[2..].join(" ");
                let pool = self.pool.read().await;

                if let Some(director) = pool.get(from) {
                    let dir = director.read().await;
                    let target = if to == "*" { None } else { Some(to) };
                    dir.send(target, TelepathyKind::Share, &message).await;
                    Ok(format!("Telepathy: {} -> {}: {}", from, to, message))
                } else {
                    Err(DirectorError::Config(format!("Director '{}' not found", from)))
                }
            }

            _ => Err(DirectorError::Config(format!("Unknown command: {}", command))),
        }
    }
}

/// Tool definition for llm-code-sdk integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectorControlToolDef {
    pub name: String,
    pub description: String,
}

impl Default for DirectorControlToolDef {
    fn default() -> Self {
        Self {
            name: "DirectorControl".to_string(),
            description: "Control Palace Directors. Commands: status, list, add <name> [model], goal <name> <desc>, issue <name> <id>, model <name> <model>, say <name> <msg>, telepathy <from> <to> <msg>".to_string(),
        }
    }
}
