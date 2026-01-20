//! Orchestrator: Director model routing via llm-code-sdk.
//!
//! Model prefixes for routing:
//! - `lm:/` = Local LM Studio (http://localhost:1234/v1)
//! - `z:/`  = Z.ai API
//! - `or:/` = OpenRouter
//! - `ms:/` = Mistral API
//!
//! Examples:
//! - `lm:/glm-4.7-flash`
//! - `z:/glm-4.7`
//! - `or:/openai/gpt-5.2`
//! - `ms:/devstral-2`
//!
//! All communication logged to Zulip.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Model route extracted from prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    /// Local LM Studio.
    Local,
    /// Z.ai API.
    Zai,
    /// OpenRouter.
    OpenRouter,
    /// Mistral API.
    Mistral,
}

impl Route {
    /// Get base URL for this route.
    pub fn base_url(&self) -> &'static str {
        match self {
            Route::Local => "http://localhost:1234/v1",
            Route::Zai => "https://api.z.ai/v1",
            Route::OpenRouter => "https://openrouter.ai/api/v1",
            Route::Mistral => "https://api.mistral.ai/v1",
        }
    }

    /// Get env var name for API key.
    pub fn api_key_env(&self) -> Option<&'static str> {
        match self {
            Route::Local => None,
            Route::Zai => Some("ZAI_API_KEY"),
            Route::OpenRouter => Some("OPENROUTER_API_KEY"),
            Route::Mistral => Some("MISTRAL_API_KEY"),
        }
    }
}

/// Parse a model string into route + model name.
///
/// Examples:
/// - `lm:/glm-4.7-flash` -> (Local, "glm-4.7-flash")
/// - `z:/glm-4.7` -> (Zai, "glm-4.7")
/// - `or:/openai/gpt-5.2` -> (OpenRouter, "openai/gpt-5.2")
/// - `glm-4.7-flash` -> (Local, "glm-4.7-flash") // default to local
pub fn parse_model(model: &str) -> (Route, &str) {
    if let Some(rest) = model.strip_prefix("lm:/") {
        (Route::Local, rest)
    } else if let Some(rest) = model.strip_prefix("z:/") {
        (Route::Zai, rest)
    } else if let Some(rest) = model.strip_prefix("or:/") {
        (Route::OpenRouter, rest)
    } else if let Some(rest) = model.strip_prefix("ms:/") {
        (Route::Mistral, rest)
    } else {
        // Default to local
        (Route::Local, model)
    }
}

/// Director configuration - just models and Zulip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectorConfig {
    /// Director ID.
    pub id: u8,
    /// Model string with route prefix.
    pub model: String,
    /// Zulip stream.
    pub zulip_stream: String,
    /// Plane workspace.
    pub plane_workspace: String,
    /// Plane project.
    pub plane_project: String,
}

impl Default for DirectorConfig {
    fn default() -> Self {
        Self {
            id: 0,
            model: "lm:/glm-4.7-flash".to_string(),
            zulip_stream: "palace".to_string(),
            plane_workspace: "wings".to_string(),
            plane_project: "PAL".to_string(),
        }
    }
}

impl DirectorConfig {
    /// Create with a specific model.
    pub fn with_model(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            ..Default::default()
        }
    }

    /// Get the route and model name.
    pub fn route(&self) -> (Route, &str) {
        parse_model(&self.model)
    }

    /// Get the base URL for this config.
    pub fn base_url(&self) -> &str {
        self.route().0.base_url()
    }
}

/// Available models (editable at runtime).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelRegistry {
    /// Model alias -> full model string.
    pub aliases: HashMap<String, String>,
    /// Default model for new directors.
    pub default: String,
}

impl ModelRegistry {
    /// Create with standard models.
    pub fn standard() -> Self {
        let mut aliases = HashMap::new();

        // Local models (LM Studio)
        aliases.insert("flash".into(), "lm:/glm-4.7-flash".into());
        aliases.insert("orch".into(), "lm:/nvidia_orchestrator-8b@q6_k_l".into());
        aliases.insert("devstral".into(), "lm:/devstral-small-2-24b-instruct-2512".into());

        // Z.ai API
        aliases.insert("glm".into(), "z:/glm-4.7".into());
        aliases.insert("glm-4.7".into(), "z:/glm-4.7".into());

        // OpenRouter
        aliases.insert("gpt".into(), "or:/openai/gpt-5.2".into());
        aliases.insert("gpt-5.2".into(), "or:/openai/gpt-5.2".into());

        // Mistral API
        aliases.insert("mistral".into(), "ms:/devstral-2".into());

        Self {
            aliases,
            default: "lm:/glm-4.7-flash".into(),
        }
    }

    /// Resolve an alias or return as-is.
    pub fn resolve(&self, model: &str) -> String {
        self.aliases.get(model).cloned().unwrap_or_else(|| model.to_string())
    }

    /// Add or update an alias.
    pub fn set_alias(&mut self, alias: &str, model: &str) {
        self.aliases.insert(alias.to_string(), model.to_string());
    }
}

/// Cached context for instant responses.
#[derive(Debug, Clone, Default)]
pub struct ContextCache {
    /// Message ID -> SmartRead summary.
    pub smart_reads: HashMap<u64, String>,
    /// Message ID -> Interview results.
    pub interviews: HashMap<u64, InterviewResult>,
    /// Issue ID -> Plane issue details.
    pub issues: HashMap<String, PlaneIssue>,
}

/// Interview result from recursive questioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterviewResult {
    pub message_id: u64,
    pub steering_prompt: Option<String>,
    pub questions: Vec<String>,
    pub answers: Vec<String>,
    pub summary: String,
}

/// Plane issue details (cached).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaneIssue {
    pub id: String,
    pub sequence_id: u32,
    pub name: String,
    pub description: Option<String>,
    pub state: String,
    pub assignee: Option<String>,
}

/// Pool of director configs.
#[derive(Debug, Clone, Default)]
pub struct DirectorPool {
    configs: Vec<DirectorConfig>,
    registry: ModelRegistry,
}

impl DirectorPool {
    /// Create with standard registry.
    pub fn new() -> Self {
        Self {
            configs: Vec::new(),
            registry: ModelRegistry::standard(),
        }
    }

    /// Add a director with a model (alias or full string).
    pub fn add(&mut self, model: &str) -> &mut Self {
        let resolved = self.registry.resolve(model);
        let mut config = DirectorConfig::with_model(resolved);
        config.id = self.configs.len() as u8;
        self.configs.push(config);
        self
    }

    /// Get configs.
    pub fn configs(&self) -> &[DirectorConfig] {
        &self.configs
    }

    /// Get registry for editing.
    pub fn registry_mut(&mut self) -> &mut ModelRegistry {
        &mut self.registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_model() {
        assert_eq!(parse_model("lm:/glm-4.7-flash"), (Route::Local, "glm-4.7-flash"));
        assert_eq!(parse_model("z:/glm-4.7"), (Route::Zai, "glm-4.7"));
        assert_eq!(parse_model("or:/openai/gpt-5.2"), (Route::OpenRouter, "openai/gpt-5.2"));
        assert_eq!(parse_model("ms:/devstral-2"), (Route::Mistral, "devstral-2"));
        assert_eq!(parse_model("glm-4.7-flash"), (Route::Local, "glm-4.7-flash")); // default
    }

    #[test]
    fn test_model_registry() {
        let reg = ModelRegistry::standard();
        assert_eq!(reg.resolve("flash"), "lm:/glm-4.7-flash");
        assert_eq!(reg.resolve("glm"), "z:/glm-4.7");
        assert_eq!(reg.resolve("gpt"), "or:/openai/gpt-5.2");
        assert_eq!(reg.resolve("unknown"), "unknown"); // passthrough
    }

    #[test]
    fn test_director_pool() {
        let mut pool = DirectorPool::new();
        pool.add("flash").add("glm").add("gpt");

        assert_eq!(pool.configs().len(), 3);
        assert_eq!(pool.configs()[0].model, "lm:/glm-4.7-flash");
        assert_eq!(pool.configs()[1].model, "z:/glm-4.7");
        assert_eq!(pool.configs()[2].model, "or:/openai/gpt-5.2");
    }

    #[test]
    fn test_route_urls() {
        assert_eq!(Route::Local.base_url(), "http://localhost:1234/v1");
        assert_eq!(Route::Zai.base_url(), "https://api.z.ai/v1");
        assert_eq!(Route::OpenRouter.base_url(), "https://openrouter.ai/api/v1");
        assert_eq!(Route::Mistral.base_url(), "https://api.mistral.ai/v1");
    }
}
