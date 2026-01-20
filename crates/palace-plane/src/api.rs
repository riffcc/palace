//! Plane.so API client.

use crate::config::{GlobalConfig, ProjectConfig};
use crate::task::PendingTask;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Plane.so API client.
pub struct PlaneClient {
    client: reqwest::Client,
    api_url: String,
    api_key: String,
}

impl PlaneClient {
    /// Create a new Plane.so client.
    pub fn new() -> Result<Self> {
        let global = GlobalConfig::load()?;
        let api_key = global.plane_api_key()
            .context("Plane.so API key not configured. Set PLANE_API_KEY or add to ~/.palace/config.yml")?;

        Ok(Self {
            client: reqwest::Client::new(),
            api_url: global.plane_url(),
            api_key,
        })
    }

    /// List active issues in a project.
    pub async fn list_active_issues(&self, config: &ProjectConfig) -> Result<Vec<PlaneIssue>> {
        let project_id = self.resolve_project_id(&config.workspace, &config.project_slug).await?;
        let url = format!(
            "{}/workspaces/{}/projects/{}/issues/",
            self.api_url, config.workspace, project_id
        );

        let response = self.client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .send()
            .await
            .context("Failed to connect to Plane.so API")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Plane.so API error: {} - {}", status, text);
        }

        let issues: PlaneIssuesResponse = response.json().await
            .context("Failed to parse Plane.so response")?;

        // Filter to active issues
        let active: Vec<PlaneIssue> = issues.results
            .into_iter()
            .filter(|i| {
                i.state.as_ref()
                    .map(|s| !s.to_lowercase().contains("done") && !s.to_lowercase().contains("cancel"))
                    .unwrap_or(true)
            })
            .collect();

        Ok(active)
    }

    /// Create an issue in Plane.so.
    pub async fn create_issue(&self, config: &ProjectConfig, task: &PendingTask) -> Result<PlaneIssue> {
        let url = format!(
            "{}/workspaces/{}/projects/{}/issues/",
            self.api_url, config.workspace, config.project_slug
        );

        let description = task.description.as_ref().map(|d| {
            let mut html = format!("<p>{}</p>", html_escape(d));

            if !task.related_files.is_empty() {
                html.push_str("<h3>Related Files</h3><ul>");
                for file in &task.related_files {
                    html.push_str(&format!("<li><code>{}</code></li>", html_escape(file)));
                }
                html.push_str("</ul>");
            }

            if let Some(effort) = &task.effort {
                html.push_str(&format!("<p><strong>Effort:</strong> {}</p>", html_escape(effort)));
            }

            html
        });

        let request = CreateIssueRequest {
            name: task.title.clone(),
            description_html: description,
            priority: Some(task.priority.as_str().to_string()),
        };

        let response = self.client
            .post(&url)
            .header("X-API-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to connect to Plane.so API")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Plane.so API error: {} - {}", status, text);
        }

        let issue: PlaneIssue = response.json().await
            .context("Failed to parse Plane.so response")?;

        Ok(issue)
    }

    async fn resolve_project_id(&self, workspace: &str, identifier: &str) -> Result<String> {
        if identifier.contains('-') && identifier.len() > 30 {
            return Ok(identifier.to_string());
        }

        let url = format!("{}/workspaces/{}/projects/", self.api_url, workspace);
        let response = self.client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to list projects");
        }

        let data: serde_json::Value = response.json().await?;
        let results = data["results"].as_array().context("No results")?;

        for project in results {
            let id = project["id"].as_str().unwrap_or("");
            let proj_identifier = project["identifier"].as_str().unwrap_or("");
            let name = project["name"].as_str().unwrap_or("");

            if proj_identifier.eq_ignore_ascii_case(identifier) || name.eq_ignore_ascii_case(identifier) {
                return Ok(id.to_string());
            }
        }

        anyhow::bail!("Project '{}' not found", identifier)
    }
}

/// Issue from Plane.so.
#[derive(Debug, Clone, Deserialize)]
pub struct PlaneIssue {
    pub id: String,
    pub sequence_id: u32,
    pub name: String,
    #[serde(default)]
    pub description_html: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlaneIssuesResponse {
    results: Vec<PlaneIssue>,
}

#[derive(Debug, Serialize)]
struct CreateIssueRequest {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description_html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<String>,
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
