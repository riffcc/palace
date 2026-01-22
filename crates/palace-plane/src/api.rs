//! Plane.so API client with rate limiting.

use crate::config::{GlobalConfig, ProjectConfig};
use crate::task::PendingTask;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::{Duration, Instant};

/// Rate limiter for Plane.so API.
/// Uses token bucket algorithm: 10 requests per second, burst of 5.
pub struct RateLimiter {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    /// Wait for a token, blocking if necessary.
    pub async fn acquire(&mut self) {
        // Refill tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;

        // If no tokens available, wait
        if self.tokens < 1.0 {
            let wait_time = (1.0 - self.tokens) / self.refill_rate;
            tokio::time::sleep(Duration::from_secs_f64(wait_time)).await;
            self.tokens = 1.0;
            self.last_refill = Instant::now();
        }

        self.tokens -= 1.0;
    }
}

/// Global rate limiter instance.
/// Plane.so allows 60 requests/minute, so we use 1.0 req/sec with burst of 5.
static RATE_LIMITER: std::sync::OnceLock<Arc<Mutex<RateLimiter>>> = std::sync::OnceLock::new();

/// Get the shared rate limiter for Plane.so API calls.
/// All code making Plane API calls should use this to avoid rate limits.
pub fn get_rate_limiter() -> Arc<Mutex<RateLimiter>> {
    RATE_LIMITER.get_or_init(|| {
        // Plane.so: 60 requests/minute = 1 req/sec average
        // We use slightly conservative rate to stay under limit
        // Burst of 5 allows quick sequences, then we pace out
        Arc::new(Mutex::new(RateLimiter::new(5.0, 0.9)))
    }).clone()
}

/// Acquire a rate limit token before making a Plane.so API call.
/// This is a convenience function that acquires from the shared limiter.
pub async fn rate_limit() {
    get_rate_limiter().lock().await.acquire().await;
}

/// Plane.so API client with built-in rate limiting.
pub struct PlaneClient {
    client: reqwest::Client,
    api_url: String,
    api_key: String,
    rate_limiter: Arc<Mutex<RateLimiter>>,
}

impl PlaneClient {
    /// Create a new Plane.so client with rate limiting.
    pub fn new() -> Result<Self> {
        let global = GlobalConfig::load()?;
        let api_key = global.plane_api_key()
            .context("Plane.so API key not configured. Set PLANE_API_KEY or add to ~/.palace/config.yml")?;

        Ok(Self {
            client: reqwest::Client::new(),
            api_url: global.plane_url(),
            api_key,
            rate_limiter: get_rate_limiter(),
        })
    }

    /// Acquire rate limit token before making request.
    async fn rate_limit(&self) {
        self.rate_limiter.lock().await.acquire().await;
    }

    /// List active issues in a project.
    pub async fn list_active_issues(&self, config: &ProjectConfig) -> Result<Vec<PlaneIssue>> {
        self.rate_limit().await;
        let project_id = self.resolve_project_id(&config.workspace, &config.project_slug).await?;

        self.rate_limit().await;
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
        self.rate_limit().await;
        let project_id = self.resolve_project_id(&config.workspace, &config.project_slug).await?;

        self.rate_limit().await;
        let url = format!(
            "{}/workspaces/{}/projects/{}/issues/",
            self.api_url, config.workspace, project_id
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

        // Note: rate_limit already called by caller
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("hello"), "hello");
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("foo & bar"), "foo &amp; bar");
        assert_eq!(html_escape("say \"hi\""), "say &quot;hi&quot;");
        assert_eq!(
            html_escape("<a href=\"test\">link & stuff</a>"),
            "&lt;a href=&quot;test&quot;&gt;link &amp; stuff&lt;/a&gt;"
        );
    }

    #[test]
    fn test_rate_limiter_burst() {
        let mut limiter = RateLimiter::new(5.0, 1.0);

        // Should have 5 tokens initially (burst capacity)
        assert!(limiter.tokens >= 4.9);

        // Consume 3 tokens synchronously (no waiting needed since we have burst)
        let rt = tokio::runtime::Runtime::new().unwrap();
        for _ in 0..3 {
            rt.block_on(limiter.acquire());
        }

        // Should have ~2 tokens left
        assert!(limiter.tokens >= 1.5 && limiter.tokens <= 2.5);
    }

    #[test]
    fn test_rate_limiter_refill() {
        let mut limiter = RateLimiter::new(5.0, 10.0); // 10 tokens/sec for fast test

        // Drain all tokens
        let rt = tokio::runtime::Runtime::new().unwrap();
        for _ in 0..5 {
            rt.block_on(limiter.acquire());
        }

        // Wait a bit for refill
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Acquire should work without long wait (tokens refilled)
        rt.block_on(limiter.acquire());

        // Should have acquired successfully (test passes if no timeout)
    }

    #[test]
    fn test_plane_issue_deserialize() {
        let json = r#"{
            "id": "abc-123",
            "sequence_id": 42,
            "name": "Fix the bug",
            "description_html": "<p>Some description</p>",
            "state": "In Progress",
            "priority": "high"
        }"#;

        let issue: PlaneIssue = serde_json::from_str(json).unwrap();

        assert_eq!(issue.id, "abc-123");
        assert_eq!(issue.sequence_id, 42);
        assert_eq!(issue.name, "Fix the bug");
        assert_eq!(issue.state, Some("In Progress".to_string()));
        assert_eq!(issue.priority, Some("high".to_string()));
    }

    #[test]
    fn test_plane_issue_deserialize_minimal() {
        // Only required fields
        let json = r#"{
            "id": "xyz-789",
            "sequence_id": 1,
            "name": "Minimal issue"
        }"#;

        let issue: PlaneIssue = serde_json::from_str(json).unwrap();

        assert_eq!(issue.id, "xyz-789");
        assert_eq!(issue.sequence_id, 1);
        assert_eq!(issue.name, "Minimal issue");
        assert!(issue.description_html.is_none());
        assert!(issue.state.is_none());
        assert!(issue.priority.is_none());
    }

    #[test]
    fn test_plane_issues_response_deserialize() {
        let json = r#"{
            "results": [
                {"id": "a", "sequence_id": 1, "name": "First"},
                {"id": "b", "sequence_id": 2, "name": "Second"}
            ]
        }"#;

        let response: PlaneIssuesResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.results.len(), 2);
        assert_eq!(response.results[0].name, "First");
        assert_eq!(response.results[1].name, "Second");
    }
}
