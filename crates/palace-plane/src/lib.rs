//! Plane.so integration and task management for Palace.
//!
//! This crate provides:
//! - Plane.so API client
//! - Task storage layer (~/.palace/projects/...)
//! - Project configuration (.palace/project.yml)
//! - Task generation and approval workflow

pub mod api;
pub mod comparison;
pub mod config;
pub mod exploration;
pub mod jecjit;
pub mod storage;
pub mod task;
pub mod velocity;

pub use api::PlaneClient;
pub use config::{GlobalConfig, ProjectConfig};
pub use jecjit::{IssueContext, JecjitContext};
pub use storage::TaskStorage;
pub use task::{ExplorationEvent, PendingTask, TaskPriority};
pub use velocity::{IssueType, VelocityTracker};

use anyhow::{Context, Result};
use llm_code_sdk::tools::ToolEventCallback;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

/// Initialize Palace for a project.
pub fn init_project(
    project_path: &Path,
    workspace: Option<&str>,
    project_slug: Option<&str>,
) -> Result<ProjectConfig> {
    let project_path = project_path.canonicalize()
        .context("Failed to resolve project path")?;

    info!("Initializing Palace for: {}", project_path.display());

    // Create .palace directory
    let palace_dir = project_path.join(".palace");
    std::fs::create_dir_all(&palace_dir)
        .context("Failed to create .palace directory")?;

    // Add .palace to .gitignore
    add_to_gitignore(&project_path)?;

    // Create or update project.yml
    let config = ProjectConfig::new_or_load(&palace_dir, workspace, project_slug)?;
    config.save(&palace_dir)?;

    Ok(config)
}

/// Generate suggestions for what to do next.
pub async fn generate_next(project_path: &Path, lm_studio_url: &str) -> Result<Vec<(u32, PendingTask)>> {
    generate_next_with_callback(project_path, lm_studio_url, None).await
}

/// Generate suggestions with an optional callback for exploration events.
pub async fn generate_next_with_callback(
    project_path: &Path,
    lm_studio_url: &str,
    on_event: Option<ToolEventCallback>,
) -> Result<Vec<(u32, PendingTask)>> {
    let project_path = project_path.canonicalize()
        .context("Failed to resolve project path")?;

    let storage = TaskStorage::new(&project_path)?;

    // Use LLM to analyze codebase and generate suggestions
    let suggestions = task::generate_suggestions_with_callback(&project_path, lm_studio_url, on_event).await?;

    if suggestions.is_empty() {
        return Ok(vec![]);
    }

    // Store as pending tasks
    let ids = storage.store_pending(&suggestions)?;

    Ok(ids.into_iter().zip(suggestions).collect())
}

/// List pending tasks.
pub fn list_pending(project_path: &Path) -> Result<Vec<(u32, PendingTask)>> {
    let project_path = project_path.canonicalize()
        .context("Failed to resolve project path")?;

    let storage = TaskStorage::new(&project_path)?;
    storage.list_pending()
}

/// List active tasks from Plane.so.
pub async fn list_active(project_path: &Path) -> Result<Vec<api::PlaneIssue>> {
    let project_path = project_path.canonicalize()
        .context("Failed to resolve project path")?;

    let config = ProjectConfig::load(&project_path)?;
    let client = PlaneClient::new()?;
    client.list_active_issues(&config).await
}

/// Remove pending tasks by display indices (1-indexed).
pub fn remove_pending(project_path: &Path, indices: &[usize]) -> Result<Vec<(u32, PendingTask)>> {
    let project_path = project_path.canonicalize()
        .context("Failed to resolve project path")?;

    let storage = TaskStorage::new(&project_path)?;
    storage.remove_pending(indices)
}

/// Approve pending tasks and create Plane.so issues.
pub async fn approve_tasks(project_path: &Path, indices: &[usize]) -> Result<Vec<(PendingTask, api::PlaneIssue)>> {
    let project_path = project_path.canonicalize()
        .context("Failed to resolve project path")?;

    let config = ProjectConfig::load(&project_path)?;
    let storage = TaskStorage::new(&project_path)?;
    let client = PlaneClient::new()?;

    let tasks = storage.get_pending_by_indices(indices)?;
    let mut results = Vec::new();

    for (id, task) in tasks {
        match client.create_issue(&config, &task).await {
            Ok(issue) => {
                storage.mark_approved(id, &issue.id)?;
                results.push((task, issue));
            }
            Err(e) => {
                tracing::error!("Failed to create issue for '{}': {}", task.title, e);
            }
        }
    }

    Ok(results)
}

/// Add .palace to .gitignore if not already present.
fn add_to_gitignore(project_path: &Path) -> Result<()> {
    let gitignore_path = project_path.join(".gitignore");

    let content = if gitignore_path.exists() {
        std::fs::read_to_string(&gitignore_path)?
    } else {
        String::new()
    };

    // Check if .palace is already ignored
    let already_ignored = content.lines().any(|line| {
        let line = line.trim();
        line == ".palace" || line == ".palace/" || line == "/.palace" || line == "/.palace/"
    });

    if !already_ignored {
        let mut new_content = content;
        if !new_content.is_empty() && !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str("\n# Palace AI assistant\n.palace/\n");
        std::fs::write(&gitignore_path, new_content)?;
        info!("Added .palace/ to .gitignore");
    }

    Ok(())
}
