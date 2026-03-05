//! Tool trait implementations for Director agent tools.
//!
//! These wrap existing functionality (ZulipTool, PlaneClient) as llm-code-sdk Tools
//! so they can be used in agentic loops.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::Arc;

use async_trait::async_trait;
use llm_code_sdk::tools::{Tool, ToolResult};
use llm_code_sdk::types::{InputSchema, PropertySchema, ToolParam};
use tokio::sync::RwLock;

use crate::ZulipTool;

/// Zulip tool for LLM agents.
///
/// Allows agents to send messages, polls, and read messages from Zulip.
pub struct ZulipAgentTool {
    inner: Arc<RwLock<Option<ZulipTool>>>,
}

impl ZulipAgentTool {
    /// Create from environment.
    pub fn from_env() -> Self {
        let tool = ZulipTool::from_env().ok();
        Self {
            inner: Arc::new(RwLock::new(tool)),
        }
    }

    /// Create with existing ZulipTool.
    pub fn new(tool: ZulipTool) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Some(tool))),
        }
    }
}

#[async_trait]
impl Tool for ZulipAgentTool {
    fn name(&self) -> &str {
        "zulip"
    }

    fn to_param(&self) -> ToolParam {
        ToolParam::new(
            "zulip",
            InputSchema::object()
                .required_string("verb", "Action: send, poll, todo, get, react")
                .optional_string("stream", "Stream name (default: palace)")
                .optional_string("topic", "Topic name")
                .optional_string("content", "Message content")
                .optional_string("question", "Poll question")
                .property("options", PropertySchema::array(PropertySchema::string()).with_description("Poll options (array of strings)"), false)
                .optional_string("title", "Todo list title")
                .property("tasks", PropertySchema::array(PropertySchema::string()).with_description("Todo tasks (array of strings)"), false)
                .property("message_id", PropertySchema::number().with_description("Message ID for reactions/updates"), false)
                .optional_string("emoji", "Emoji name for reactions")
                .property("limit", PropertySchema::number().with_description("Number of messages to fetch (default: 10)"), false),
        )
        .with_description(
            "Zulip messaging tool. Verbs: send (message), poll (create poll), todo (create todo list), get (fetch messages), react (add reaction)",
        )
    }

    async fn call(&self, input: HashMap<String, serde_json::Value>) -> ToolResult {
        let guard = self.inner.read().await;
        let Some(tool) = guard.as_ref() else {
            return ToolResult::error("Zulip not configured (missing ZULIP_* env vars)");
        };

        let verb = input
            .get("verb")
            .and_then(|v| v.as_str())
            .unwrap_or("send");
        let stream = input
            .get("stream")
            .and_then(|v| v.as_str())
            .unwrap_or("palace");
        let topic = input.get("topic").and_then(|v| v.as_str()).unwrap_or("");

        match verb {
            "send" => {
                let content = input
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if content.is_empty() {
                    return ToolResult::error("content is required for send");
                }
                match tool.send(stream, topic, content).await {
                    Ok(id) => ToolResult::success(format!("Message sent (id: {})", id)),
                    Err(e) => ToolResult::error(format!("Failed to send: {}", e)),
                }
            }

            "poll" => {
                let question = input
                    .get("question")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let options: Vec<&str> = input
                    .get("options")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .collect()
                    })
                    .unwrap_or_default();

                if question.is_empty() || options.is_empty() {
                    return ToolResult::error("question and options are required for poll");
                }

                match tool.send_poll(stream, topic, question, &options).await {
                    Ok(id) => ToolResult::success(format!("Poll created (id: {})", id)),
                    Err(e) => ToolResult::error(format!("Failed to create poll: {}", e)),
                }
            }

            "todo" => {
                let title = input.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let tasks: Vec<&str> = input
                    .get("tasks")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .collect()
                    })
                    .unwrap_or_default();

                if title.is_empty() || tasks.is_empty() {
                    return ToolResult::error("title and tasks are required for todo");
                }

                match tool.send_todo(stream, topic, title, &tasks).await {
                    Ok(id) => ToolResult::success(format!("Todo list created (id: {})", id)),
                    Err(e) => ToolResult::error(format!("Failed to create todo: {}", e)),
                }
            }

            "get" => {
                let limit = input
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10) as u32;
                let topic_opt = if topic.is_empty() { None } else { Some(topic) };

                match tool.get_messages(stream, topic_opt, limit).await {
                    Ok(messages) => {
                        let formatted: Vec<String> = messages
                            .iter()
                            .map(|m| {
                                format!(
                                    "[{}] {}: {}",
                                    m.subject,
                                    m.sender_full_name,
                                    m.text()
                                )
                            })
                            .collect();
                        ToolResult::success(formatted.join("\n\n"))
                    }
                    Err(e) => ToolResult::error(format!("Failed to get messages: {}", e)),
                }
            }

            "react" => {
                let message_id = input
                    .get("message_id")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let emoji = input.get("emoji").and_then(|v| v.as_str()).unwrap_or("");

                if message_id == 0 || emoji.is_empty() {
                    return ToolResult::error("message_id and emoji are required for react");
                }

                match tool.add_reaction(message_id, emoji).await {
                    Ok(()) => ToolResult::success(format!("Reaction {} added", emoji)),
                    Err(e) => ToolResult::error(format!("Failed to add reaction: {}", e)),
                }
            }

            _ => ToolResult::error(format!(
                "Unknown verb '{}'. Use: send, poll, todo, get, react",
                verb
            )),
        }
    }
}

/// Plane.so tool for LLM agents.
///
/// Allows agents to list, create, and update issues.
pub struct PlaneAgentTool {
    workspace: String,
    project: String,
}

impl PlaneAgentTool {
    /// Create with workspace and project.
    pub fn new(workspace: &str, project: &str) -> Self {
        Self {
            workspace: workspace.to_string(),
            project: project.to_string(),
        }
    }

    /// Create from environment or defaults.
    pub fn from_env() -> Self {
        let workspace = std::env::var("PLANE_WORKSPACE_SLUG")
            .or_else(|_| std::env::var("PLANE_WORKSPACE"))
            .unwrap_or_else(|_| "wings".to_string());
        let project = std::env::var("PLANE_PROJECT").unwrap_or_else(|_| "PAL".to_string());
        Self { workspace, project }
    }
}

#[async_trait]
impl Tool for PlaneAgentTool {
    fn name(&self) -> &str {
        "plane"
    }

    fn to_param(&self) -> ToolParam {
        ToolParam::new(
            "plane",
            InputSchema::object()
                .required_string("verb", "Action: list, get, create, update")
                .optional_string("project", "Project identifier (default from env)")
                .optional_string("id", "Issue ID for get/update")
                .optional_string("name", "Issue name for create")
                .optional_string("description", "Issue description")
                .optional_string("priority", "Priority: urgent, high, medium, low, none")
                .optional_string("state", "State to set"),
        )
        .with_description("Plane.so issue management. Verbs: list, get, create, update")
    }

    async fn call(&self, input: HashMap<String, serde_json::Value>) -> ToolResult {
        let verb = input
            .get("verb")
            .and_then(|v| v.as_str())
            .unwrap_or("list");
        let project = input
            .get("project")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.project);

        // Use palace call plane internally to avoid duplicating API logic.
        let mut args = serde_json::json!({
            "verb": verb,
            "project": project,
        });

        // Copy optional fields
        for key in ["id", "name", "description", "priority", "state"] {
            if let Some(val) = input.get(key) {
                args[key] = val.clone();
            }
        }

        // Prefer `palace`; fallback to legacy `pal`.
        for cli in ["palace", "pal"] {
            let mut cmd = tokio::process::Command::new(cli);
            cmd.arg("call")
                .arg("plane")
                .arg("--input")
                .arg(args.to_string());

            // Forward Plane auth/workspace env explicitly so child processes
            // can use the same credentials when available.
            if let Ok(api_key) = std::env::var("PLANE_API_KEY") {
                cmd.env("PLANE_API_KEY", api_key);
            }
            let workspace_slug = std::env::var("PLANE_WORKSPACE_SLUG")
                .or_else(|_| std::env::var("PLANE_WORKSPACE"))
                .unwrap_or_else(|_| self.workspace.clone());
            cmd.env("PLANE_WORKSPACE_SLUG", &workspace_slug);
            cmd.env("PLANE_WORKSPACE", &workspace_slug);
            cmd.env("PLANE_PROJECT", &self.project);

            let output = cmd.output().await;

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if out.status.success() {
                        return ToolResult::success(stdout.to_string());
                    }
                    return ToolResult::error(format!("{}\n{}", stdout, stderr));
                }
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => {
                    return ToolResult::error(format!("Failed to call {}: {}", cli, e));
                }
            }
        }

        ToolResult::error("Failed to call Plane CLI: neither `palace` nor `pal` found in PATH")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zulip_tool_param() {
        let tool = ZulipAgentTool::from_env();
        let param = tool.to_param();
        assert_eq!(param.name, "zulip");
    }

    #[test]
    fn test_plane_tool_param() {
        let tool = PlaneAgentTool::from_env();
        let param = tool.to_param();
        assert_eq!(param.name, "plane");
    }
}
