//! Exploration tools for agentic codebase analysis.
//!
//! Uses standard tools from llm-code-sdk (read_file, glob, grep, list_directory).
//! Model outputs suggestions as JSON in its final message - no special tool needed.

use std::path::Path;
use std::sync::Arc;

use llm_code_sdk::tools::Tool;

/// Create exploration tools for a project.
///
/// Returns standard exploration tools from llm-code-sdk.
/// The model outputs suggestions directly as JSON - no suggest tool.
pub fn create_exploration_tools(project_root: &Path) -> Vec<Arc<dyn Tool>> {
    llm_code_sdk::create_exploration_tools(project_root)
}
