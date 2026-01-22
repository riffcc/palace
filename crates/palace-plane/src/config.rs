//! Configuration for Palace projects.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Credentials stored in ~/.palace/credentials.json
///
/// Lookup order: credentials.json first, then env var
/// This allows local config while Docker/.env still works.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Credentials {
    /// Z.ai API key
    #[serde(default)]
    pub zai_api_key: Option<String>,

    /// Plane.so API key
    #[serde(default)]
    pub plane_api_key: Option<String>,

    /// OpenRouter API key (for premium models)
    #[serde(default)]
    pub openrouter_api_key: Option<String>,

    /// Zulip server URL
    #[serde(default)]
    pub zulip_server_url: Option<String>,

    /// Zulip insecure mode (accept invalid certs)
    #[serde(default)]
    pub zulip_insecure: Option<bool>,

    /// Palace bot email
    #[serde(default)]
    pub palace_bot_email: Option<String>,

    /// Palace bot API key
    #[serde(default)]
    pub palace_api_key: Option<String>,

    /// Director bot email
    #[serde(default)]
    pub director_bot_email: Option<String>,

    /// Director bot API key
    #[serde(default)]
    pub director_api_key: Option<String>,
}

impl Credentials {
    /// Load credentials from ~/.palace/credentials.json
    pub fn load() -> Result<Self> {
        let home = dirs::home_dir().context("Could not find home directory")?;
        let creds_path = home.join(".palace").join("credentials.json");

        if creds_path.exists() {
            let content = std::fs::read_to_string(&creds_path)
                .context("Failed to read credentials")?;
            serde_json::from_str(&content).context("Failed to parse credentials")
        } else {
            Ok(Self::default())
        }
    }

    /// Save credentials to ~/.palace/credentials.json
    pub fn save(&self) -> Result<()> {
        let home = dirs::home_dir().context("Could not find home directory")?;
        let palace_dir = home.join(".palace");
        std::fs::create_dir_all(&palace_dir)?;

        let creds_path = palace_dir.join("credentials.json");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&creds_path, content)?;

        Ok(())
    }

    /// Get Z.ai API key (credentials.json first, then env var).
    pub fn zai_api_key(&self) -> Option<String> {
        self.zai_api_key.clone()
            .or_else(|| std::env::var("ZAI_API_KEY").ok())
    }

    /// Get Plane.so API key (credentials.json first, then env var).
    pub fn plane_api_key(&self) -> Option<String> {
        self.plane_api_key.clone()
            .or_else(|| std::env::var("PLANE_API_KEY").ok())
    }

    /// Get OpenRouter API key (credentials.json first, then env var).
    pub fn openrouter_api_key(&self) -> Option<String> {
        self.openrouter_api_key.clone()
            .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
    }

    /// Get Zulip server URL.
    pub fn zulip_server_url(&self) -> Option<String> {
        self.zulip_server_url.clone()
            .or_else(|| std::env::var("ZULIP_SERVER_URL").ok())
    }

    /// Get Zulip insecure mode.
    pub fn zulip_insecure(&self) -> bool {
        self.zulip_insecure.unwrap_or(false)
            || std::env::var("ZULIP_INSECURE")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false)
    }

    /// Get Palace bot email.
    pub fn palace_bot_email(&self) -> Option<String> {
        self.palace_bot_email.clone()
            .or_else(|| std::env::var("PALACE_BOT_EMAIL").ok())
            .or_else(|| std::env::var("ZULIP_BOT_EMAIL").ok())
    }

    /// Get Palace bot API key.
    pub fn palace_api_key(&self) -> Option<String> {
        self.palace_api_key.clone()
            .or_else(|| std::env::var("PALACE_API_KEY").ok())
            .or_else(|| std::env::var("ZULIP_API_KEY").ok())
    }

    /// Get Director bot email.
    pub fn director_bot_email(&self) -> Option<String> {
        self.director_bot_email.clone()
            .or_else(|| std::env::var("DIRECTOR_BOT_EMAIL").ok())
            .or_else(|| std::env::var("ZULIP_BOT_EMAIL").ok())
    }

    /// Get Director bot API key.
    pub fn director_api_key(&self) -> Option<String> {
        self.director_api_key.clone()
            .or_else(|| std::env::var("DIRECTOR_API_KEY").ok())
            .or_else(|| std::env::var("ZULIP_API_KEY").ok())
    }
}

/// Global Palace configuration (~/.palace/config.yml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Default Plane.so workspace.
    #[serde(default)]
    pub plane_default_workspace: Option<String>,

    /// Plane.so API key (deprecated - use credentials.json).
    #[serde(default)]
    pub plane_api_key: Option<String>,

    /// Plane.so API URL.
    #[serde(default = "default_plane_url")]
    pub plane_api_url: String,

    /// Folders to scan for projects (looks for .palace/project.yml inside subdirs).
    #[serde(default)]
    pub project_folders: Vec<String>,
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
            project_folders: Vec::new(),
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

    /// Get Plane.so API key (credentials.json first, then config, then env var).
    pub fn plane_api_key(&self) -> Option<String> {
        Credentials::load().ok()
            .and_then(|c| c.plane_api_key.clone())
            .or_else(|| self.plane_api_key.clone())
            .or_else(|| std::env::var("PLANE_API_KEY").ok())
    }

    /// Get Plane.so API URL (from config or environment).
    pub fn plane_url(&self) -> String {
        std::env::var("PLANE_API_URL")
            .unwrap_or_else(|_| self.plane_api_url.clone())
    }

    /// Discover all projects by scanning configured folders.
    /// Returns map of stream name -> project path.
    pub fn discover_projects(&self) -> std::collections::HashMap<String, std::path::PathBuf> {
        let mut lookup = std::collections::HashMap::new();

        for folder in &self.project_folders {
            let folder_path = std::path::PathBuf::from(folder);
            if let Ok(entries) = std::fs::read_dir(&folder_path) {
                for entry in entries.flatten() {
                    let project_path = entry.path();
                    if project_path.is_dir() {
                        // Check if this dir has .palace/project.yml
                        if let Ok(config) = ProjectConfig::load(&project_path) {
                            // Use the name field as stream name, fall back to project_slug
                            let stream_name = config.name
                                .unwrap_or_else(|| config.project_slug.clone());
                            lookup.insert(stream_name, project_path);
                        }
                    }
                }
            }
        }

        lookup
    }

    /// Find project path by stream name.
    pub fn find_project_by_stream(&self, stream: &str) -> Option<std::path::PathBuf> {
        self.discover_projects().get(stream).cloned()
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
