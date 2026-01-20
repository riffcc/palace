//! Exploration tools for agentic codebase analysis.
//!
//! This module provides the SuggestTool for structured output.
//! Standard tools (read_file, glob, grep, etc.) come from llm-code-sdk.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use llm_code_sdk::tools::{Tool, ToolResult};
use llm_code_sdk::types::{InputSchema, PropertySchema, ToolParam};

/// Tool for outputting the final suggestions.
pub struct SuggestTool;

#[async_trait]
impl Tool for SuggestTool {
    fn name(&self) -> &str {
        "suggest"
    }

    fn to_param(&self) -> ToolParam {
        ToolParam::new(
            "suggest",
            InputSchema::object().property(
                "suggestions",
                PropertySchema::array(
                    PropertySchema::object()
                        .property(
                            "title",
                            PropertySchema::string()
                                .with_description("Task title (max 240 chars)"),
                            true,
                        )
                        .property(
                            "description",
                            PropertySchema::string().with_description("What to do and why"),
                            true,
                        ),
                ),
                true,
            ),
        )
        .with_description(
            "Output your suggestions after exploring the codebase. Each suggestion needs a title and description.",
        )
    }

    async fn call(&self, input: HashMap<String, serde_json::Value>) -> ToolResult {
        match serde_json::to_string_pretty(&input) {
            Ok(json) => ToolResult::success(format!("SUGGESTIONS_OUTPUT:{}", json)),
            Err(e) => ToolResult::error(format!("Invalid suggestions format: {}", e)),
        }
    }
}

/// Create exploration tools for a project (uses llm-code-sdk standard tools + SuggestTool).
pub fn create_exploration_tools(project_root: &Path) -> Vec<Arc<dyn Tool>> {
    let mut tools = llm_code_sdk::create_exploration_tools(project_root);
    tools.push(Arc::new(SuggestTool) as Arc<dyn Tool>);
    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_suggest_tool() {
        let tool = SuggestTool;
        let mut input = HashMap::new();
        input.insert(
            "suggestions".to_string(),
            serde_json::json!([
                {"title": "Test task", "description": "A test description"}
            ]),
        );

        let result = tool.call(input).await;
        assert!(!result.is_error());
        assert!(result.to_content_string().contains("SUGGESTIONS_OUTPUT:"));
    }
}
