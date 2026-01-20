//! Configuration for Palace projects.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Global Palace configuration (~/.palace/config.yml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Default Plane.so workspace.
    #[serde(default)]
    pub plane_default_workspace: Option<String>,

    /// Plane.so API key.
    #[serde(default)]
    pub plane_api_key: Option<String>,

    /// Plane.so API URL.
    #[serde(default = "default_plane_url")]
    pub plane_api_url: String,
}

fn default_plane_url() -> String {
    "https://api.plane.so/api/v1".to_string()
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            plane_default_workspace: None,
            plane_api_key: None,
            plane_api_url: default_plane_url(),
        }
    }
}

impl GlobalConfig {
    /// Load global config from ~/.palace/config.yml.
    pub fn load() -> Result<Self> {
        let home = dirs::home_dir().context("Could not find home directory")?;
        let config_path = home.join(".palace").join("config.yml");

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .context("Failed to read global config")?;
            serde_yaml::from_str(&content).context("Failed to parse global config")
        } else {
            Ok(Self::default())
        }
    }

    /// Save global config.
    pub fn save(&self) -> Result<()> {
        let home = dirs::home_dir().context("Could not find home directory")?;
        let palace_dir = home.join(".palace");
        std::fs::create_dir_all(&palace_dir)?;

        let config_path = palace_dir.join("config.yml");
        let content = serde_yaml::to_string(self)?;
        std::fs::write(&config_path, content)?;

        Ok(())
    }

    /// Get Plane.so API key (from config or environment).
    pub fn plane_api_key(&self) -> Option<String> {
        self.plane_api_key.clone()
            .or_else(|| std::env::var("PLANE_API_KEY").ok())
    }

    /// Get Plane.so API URL (from config or environment).
    pub fn plane_url(&self) -> String {
        std::env::var("PLANE_API_URL")
            .unwrap_or_else(|_| self.plane_api_url.clone())
    }
}

/// Project-specific configuration (.palace/project.yml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Plane.so workspace slug.
    pub workspace: String,

    /// Plane.so project slug.
    pub project_slug: String,

    /// Project display name.
    #[serde(default)]
    pub name: Option<String>,

    /// Spec files to compare against (for gap detection).
    #[serde(default)]
    pub spec_files: Vec<String>,
}

impl ProjectConfig {
    /// Create new or load existing project config.
    pub fn new_or_load(
        palace_dir: &Path,
        workspace: Option<&str>,
        project_slug: Option<&str>,
    ) -> Result<Self> {
        let config_path = palace_dir.join("project.yml");

        if config_path.exists() {
            let mut config = Self::load_from(&config_path)?;
            if let Some(ws) = workspace {
                config.workspace = ws.to_string();
            }
            if let Some(slug) = project_slug {
                config.project_slug = slug.to_string();
            }
            Ok(config)
        } else {
            let global = GlobalConfig::load()?;

            let workspace = workspace
                .map(String::from)
                .or(global.plane_default_workspace)
                .unwrap_or_else(|| "default".to_string());

            let project_slug = project_slug
                .map(String::from)
                .unwrap_or_else(|| {
                    palace_dir
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .map(|s| slug_from_name(s))
                        .unwrap_or_else(|| "project".to_string())
                });

            Ok(Self {
                workspace,
                project_slug,
                name: None,
                spec_files: Vec::new(),
            })
        }
    }

    /// Load project config from project directory.
    pub fn load(project_path: &Path) -> Result<Self> {
        let config_path = project_path.join(".palace").join("project.yml");
        Self::load_from(&config_path)
    }

    fn load_from(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read project config at {}. Run 'palace init' first.", path.display()))?;
        serde_yaml::from_str(&content).context("Failed to parse project config")
    }

    /// Save project config.
    pub fn save(&self, palace_dir: &Path) -> Result<()> {
        let config_path = palace_dir.join("project.yml");
        let content = serde_yaml::to_string(self)?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }
}

/// Generate a slug from a project name.
fn slug_from_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
