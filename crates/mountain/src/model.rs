//! Model tier configuration for the cascade.

use serde::{Deserialize, Serialize};

/// Type of model - local or cloud.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    /// Local model (llama.cpp, ollama, etc.)
    Local,
    /// Cloud API model
    Cloud,
}

/// Configuration for a model tier in the cascade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model identifier/name.
    pub model_id: String,

    /// API endpoint (for cloud) or local server URL.
    pub endpoint: Option<String>,

    /// API key (for cloud models).
    pub api_key: Option<String>,

    /// Maximum tokens to generate.
    pub max_tokens: u32,

    /// Temperature for generation.
    pub temperature: f32,

    /// System prompt for this tier.
    pub system_prompt: Option<String>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model_id: String::new(),
            endpoint: None,
            api_key: None,
            max_tokens: 512,
            temperature: 0.7,
            system_prompt: None,
        }
    }
}

/// A model tier in the cascade hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTier {
    /// Tier name (for logging/debugging).
    pub name: String,

    /// Model type.
    pub model_type: ModelType,

    /// Model configuration.
    pub config: ModelConfig,

    /// Expected latency in milliseconds.
    pub expected_latency_ms: u64,

    /// Whether this tier should be "frozen" until prior tiers complete.
    /// A frozen tier receives input from all prior tiers before responding.
    pub frozen: bool,

    /// Priority weight for decision merging (higher = more influence).
    pub priority: u32,

    /// Whether this tier can veto decisions from lower-priority tiers.
    pub can_veto: bool,
}

impl ModelTier {
    /// Create a new local model tier.
    pub fn local(model_id: impl Into<String>, expected_latency_ms: u64) -> Self {
        let model_id = model_id.into();
        Self {
            name: model_id.clone(),
            model_type: ModelType::Local,
            config: ModelConfig {
                model_id,
                endpoint: Some("http://localhost:11434".into()), // Default ollama
                ..Default::default()
            },
            expected_latency_ms,
            frozen: false,
            priority: 1,
            can_veto: false,
        }
    }

    /// Create a new cloud model tier.
    pub fn cloud(model_id: impl Into<String>, expected_latency_ms: u64) -> Self {
        let model_id = model_id.into();
        Self {
            name: model_id.clone(),
            model_type: ModelType::Cloud,
            config: ModelConfig {
                model_id,
                ..Default::default()
            },
            expected_latency_ms,
            frozen: false,
            priority: 1,
            can_veto: false,
        }
    }

    /// Set this tier as frozen (waits for prior tier results).
    pub fn frozen(mut self) -> Self {
        self.frozen = true;
        self
    }

    /// Set the priority weight.
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Enable veto power.
    pub fn with_veto(mut self) -> Self {
        self.can_veto = true;
        self
    }

    /// Set the endpoint.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config.endpoint = Some(endpoint.into());
        self
    }

    /// Set the API key.
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.config.api_key = Some(key.into());
        self
    }

    /// Set max tokens.
    pub fn with_max_tokens(mut self, max: u32) -> Self {
        self.config.max_tokens = max;
        self
    }

    /// Set temperature.
    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.config.temperature = temp;
        self
    }

    /// Set system prompt.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.config.system_prompt = Some(prompt.into());
        self
    }

    /// Check if this is a local model.
    pub fn is_local(&self) -> bool {
        self.model_type == ModelType::Local
    }

    /// Check if this is a cloud model.
    pub fn is_cloud(&self) -> bool {
        self.model_type == ModelType::Cloud
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_tier() {
        let tier = ModelTier::local("nvidia_orchestrator-8b", 100)
            .with_priority(1)
            .with_max_tokens(256);

        assert!(tier.is_local());
        assert_eq!(tier.expected_latency_ms, 100);
        assert_eq!(tier.config.max_tokens, 256);
    }

    #[test]
    fn test_cloud_tier_frozen() {
        let tier = ModelTier::cloud("glm-4.7", 2000)
            .frozen()
            .with_veto()
            .with_priority(10);

        assert!(tier.is_cloud());
        assert!(tier.frozen);
        assert!(tier.can_veto);
        assert_eq!(tier.priority, 10);
    }
}
