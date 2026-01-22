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
pub mod suggestions;
pub mod task;
pub mod velocity;

pub use api::{PlaneClient, rate_limit, get_rate_limiter};
pub use config::{Credentials, GlobalConfig, ProjectConfig};
pub use jecjit::{IssueContext, JecjitContext};
pub use storage::TaskStorage;
pub use suggestions::{StoredSuggestion, append_suggestions, load_suggestions, remove_suggestions};
pub use task::{ExplorationEvent, PendingTask, TaskPriority, TaskRelation, RelationType, EnhancementCallback, enhance_suggestion, enhance_all_suggestions};
pub use velocity::{IssueType, VelocityTracker};

use anyhow::{Context, Result};
use llm_code_sdk::client::Client;
use llm_code_sdk::tools::ToolEventCallback;
use llm_code_sdk::types::{MessageCreateParams, MessageParam};
use std::path::Path;
use tracing::info;

/// Result of smart project initialization.
pub struct SmartInitResult {
    pub config: ProjectConfig,
    pub created_new: bool,
    pub matched_existing: Option<String>,
}

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

/// Smart JIT initialization - matches existing projects or creates new ones.
///
/// This is called automatically by any `pal` command that needs project context.
/// Uses LLM with structured output to intelligently match or generate project name/slug.
pub async fn smart_init_project(project_path: &Path) -> Result<SmartInitResult> {
    let project_path = project_path.canonicalize()
        .context("Failed to resolve project path")?;

    let palace_dir = project_path.join(".palace");
    let config_path = palace_dir.join("project.yml");

    // If config exists, just load and return
    if config_path.exists() {
        let config = ProjectConfig::load(&project_path)?;
        return Ok(SmartInitResult {
            config,
            created_new: false,
            matched_existing: None,
        });
    }

    eprintln!("Generating Palace configuration...");

    // Get global config for default workspace
    let global = GlobalConfig::load()?;
    let workspace = global.plane_default_workspace
        .as_deref()
        .unwrap_or("wings");

    // Get directory name and any README/CLAUDE.md content for context
    let dir_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let mut project_context = format!("Directory: {}\n", dir_name);

    // Read README or CLAUDE.md if present for more context
    for readme in ["CLAUDE.md", "README.md", "README"] {
        let readme_path = project_path.join(readme);
        if readme_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&readme_path) {
                let preview: String = content.lines().take(30).collect::<Vec<_>>().join("\n");
                project_context.push_str(&format!("\n{}:\n{}\n", readme, preview));
                break;
            }
        }
    }

    // Also check for Cargo.toml or package.json
    for manifest in ["Cargo.toml", "package.json"] {
        let manifest_path = project_path.join(manifest);
        if manifest_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                let preview: String = content.lines().take(15).collect::<Vec<_>>().join("\n");
                project_context.push_str(&format!("\n{}:\n{}\n", manifest, preview));
                break;
            }
        }
    }

    // Fetch existing projects from Plane
    let api_key = global.plane_api_key()
        .context("PLANE_API_KEY not set")?;
    let api_url = global.plane_url();

    let http_client = reqwest::Client::new();
    let url = format!("{}/workspaces/{}/projects/", api_url, workspace);

    let resp: serde_json::Value = http_client.get(&url)
        .header("X-API-Key", &api_key)
        .send()
        .await
        .context("Failed to fetch projects from Plane")?
        .json()
        .await
        .context("Failed to parse projects response")?;

    let existing_projects: Vec<(String, String)> = resp["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    Some((
                        p["identifier"].as_str()?.to_string(),
                        p["name"].as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let existing_list = if existing_projects.is_empty() {
        "None".to_string()
    } else {
        existing_projects
            .iter()
            .map(|(slug, name)| format!("{} ({})", slug, name))
            .collect::<Vec<_>>()
            .join(", ")
    };

    // Use structured output to get project config from LLM (Z.ai API)
    let zai_key = std::env::var("ZAI_API_KEY")
        .context("ZAI_API_KEY not set")?;
    let llm_client = Client::zai(&zai_key)
        .context("Failed to create Z.ai client")?;

    let system = r#"Configure Plane.so project association.
Return {"match": "SLUG"} to use an existing project, or {"name": "Project Name", "slug": "SLUG"} for a new one."#;

    let prompt = format!(
        "Project:\n{project_context}\n\nExisting projects: {existing_list}"
    );

    let params = MessageCreateParams {
        model: "glm-4.7".to_string(),
        system: Some(llm_code_sdk::types::SystemPrompt::Text(system.to_string())),
        messages: vec![MessageParam::user(&prompt)],
        response_format: Some(llm_code_sdk::types::ResponseFormat::json_object()),
        ..Default::default()
    };

    let message = llm_client.messages().create(params).await
        .context("Failed to get LLM response")?;

    let response_text = message.text().unwrap_or_default();

    // Parse the JSON response
    let parsed: serde_json::Value = serde_json::from_str(&response_text)
        .context("Failed to parse LLM JSON response")?;

    let match_existing = parsed.get("match").and_then(|v| v.as_str()).map(|s| s.to_string());
    let name = parsed.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
    let slug = parsed.get("slug").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Create .palace directory
    std::fs::create_dir_all(&palace_dir)
        .context("Failed to create .palace directory")?;
    add_to_gitignore(&project_path)?;

    let (config, created_new, matched_existing_result) = if let Some(matched_slug) = match_existing {
        // Use existing project
        let project_name = existing_projects
            .iter()
            .find(|(s, _)| s == &matched_slug)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| dir_name.to_string());

        eprintln!("Name: {}", project_name);
        eprintln!("Slug: {} (matched existing project)", matched_slug);

        let config = ProjectConfig {
            workspace: workspace.to_string(),
            project_slug: matched_slug.clone(),
            name: Some(project_name),
            spec_files: Vec::new(),
        };
        (config, false, Some(matched_slug))
    } else {
        // Create new project
        let project_name = name.unwrap_or_else(|| dir_name.to_string());
        let project_slug = slug.unwrap_or_else(|| "PRJ".to_string()).to_uppercase();

        eprintln!("Name: {}", project_name);
        eprintln!("Slug: {}", project_slug);

        // Create project in Plane
        let body = serde_json::json!({
            "name": project_name,
            "identifier": project_slug,
            "network": 2
        });

        let create_resp = http_client.post(&url)
            .header("X-API-Key", &api_key)
            .json(&body)
            .send()
            .await;

        match create_resp {
            Ok(resp) if resp.status().is_success() => {}
            Ok(resp) => {
                let text = resp.text().await.unwrap_or_default();
                if !text.contains("already exists") {
                    tracing::warn!("Failed to create project in Plane: {}", text);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to create project in Plane: {}", e);
            }
        }

        let config = ProjectConfig {
            workspace: workspace.to_string(),
            project_slug,
            name: Some(project_name),
            spec_files: Vec::new(),
        };
        (config, true, None)
    };

    config.save(&palace_dir)?;
    eprintln!("Run \"pal config\" to edit if needed.\n");

    Ok(SmartInitResult {
        config,
        created_new,
        matched_existing: matched_existing_result,
    })
}

/// Generate suggestions for what to do next.
///
/// Uses Z.ai API with glm-4.7 by default (requires ZAI_API_KEY env var).
pub async fn generate_next(project_path: &Path) -> Result<Vec<StoredSuggestion>> {
    generate_next_with_options(project_path, None, None, None).await
}

/// Generate suggestions with optional callback, request count, and endpoint override.
///
/// - `on_event`: Optional callback for exploration events
/// - `request_up_to`: Optional guidance for how many tasks needed (not a constraint)
/// - `lm_studio_url`: Optional LM Studio URL (uses Z.ai API if None)
pub async fn generate_next_with_options(
    project_path: &Path,
    on_event: Option<ToolEventCallback>,
    request_up_to: Option<usize>,
    lm_studio_url: Option<&str>,
) -> Result<Vec<StoredSuggestion>> {
    let project_path = project_path.canonicalize()
        .context("Failed to resolve project path")?;

    // Use LLM to analyze codebase and generate suggestions
    let suggestions = task::generate_suggestions_with_options(&project_path, on_event, request_up_to, lm_studio_url).await?;

    if suggestions.is_empty() {
        return Ok(vec![]);
    }

    // Store suggestions atomically to .palace/suggestions.json
    let stored = append_suggestions(&project_path, suggestions)?;

    Ok(stored)
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

/// Approve suggestions and create Plane.so issues.
pub async fn approve_tasks(project_path: &Path, indices: &[usize]) -> Result<Vec<(PendingTask, api::PlaneIssue)>> {
    let project_path = project_path.canonicalize()
        .context("Failed to resolve project path")?;

    let config = ProjectConfig::load(&project_path)?;
    let client = PlaneClient::new()?;

    // Load suggestions and filter by indices
    let suggestions = load_suggestions(&project_path)?;
    let to_approve: Vec<_> = suggestions
        .iter()
        .filter(|s| indices.contains(&s.index))
        .collect();

    let mut results = Vec::new();
    let mut approved_indices = Vec::new();

    for s in to_approve {
        match client.create_issue(&config, &s.task).await {
            Ok(issue) => {
                approved_indices.push(s.index);
                results.push((s.task.clone(), issue));
            }
            Err(e) => {
                tracing::error!("Failed to create issue for '{}': {}", s.task.title, e);
            }
        }
    }

    // Remove approved suggestions from the file
    if !approved_indices.is_empty() {
        remove_suggestions(&project_path, &approved_indices)?;
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
