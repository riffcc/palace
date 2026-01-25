//! ModelLadder - self-upgrading model selection.
//!
//! Agents can request to upgrade themselves to more capable models
//! when they recognize task complexity exceeds their capabilities.
//!
//! ## Model Tiers
//!
//! ```text
//! FAST (8b local)
//!   ↓
//! FLASH (flash local)
//!   ↓
//! MINI (ministral-3-14b local)
//!   ↓
//! CODE (devstral-2 remote)
//!   ↓
//! FULL (glm-4.7 remote)
//!   ↓
//! CLOUD (gpt-5.2 / opus remote)
//! ```
//!
//! Agents can self-elect to climb the ladder or request specific models.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Model capability tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    #[default]
    /// Ultra-fast local 8B model for quick decisions.
    Fast,
    /// Fast local flash model.
    Flash,
    /// Local reasoning model (14B).
    Mini,
    /// Fast code generation (Devstral 2).
    Code,
    /// Full capability local/remote (GLM-4.7).
    Full,
    /// Premium cloud models (GPT-5.2, Opus).
    Cloud,
}

impl ModelTier {
    /// Get all tiers in order.
    pub fn all() -> &'static [ModelTier] {
        &[
            ModelTier::Fast,
            ModelTier::Flash,
            ModelTier::Mini,
            ModelTier::Code,
            ModelTier::Full,
            ModelTier::Cloud,
        ]
    }

    /// Get the next tier up.
    pub fn upgrade(&self) -> Option<ModelTier> {
        match self {
            ModelTier::Fast => Some(ModelTier::Flash),
            ModelTier::Flash => Some(ModelTier::Mini),
            ModelTier::Mini => Some(ModelTier::Code),
            ModelTier::Code => Some(ModelTier::Full),
            ModelTier::Full => Some(ModelTier::Cloud),
            ModelTier::Cloud => None,
        }
    }

    /// Get the next tier down.
    pub fn downgrade(&self) -> Option<ModelTier> {
        match self {
            ModelTier::Fast => None,
            ModelTier::Flash => Some(ModelTier::Fast),
            ModelTier::Mini => Some(ModelTier::Flash),
            ModelTier::Code => Some(ModelTier::Mini),
            ModelTier::Full => Some(ModelTier::Code),
            ModelTier::Cloud => Some(ModelTier::Full),
        }
    }

    /// Check if this tier is local.
    pub fn is_local(&self) -> bool {
        matches!(self, ModelTier::Fast | ModelTier::Flash | ModelTier::Mini)
    }

    /// Check if this tier is remote/cloud.
    pub fn is_remote(&self) -> bool {
        !self.is_local()
    }

    /// Estimated cost factor (relative to Fast = 1.0).
    pub fn cost_factor(&self) -> f32 {
        match self {
            ModelTier::Fast => 1.0,
            ModelTier::Flash => 1.5,
            ModelTier::Mini => 3.0,
            ModelTier::Code => 10.0,  // Remote, but fast
            ModelTier::Full => 20.0,  // Remote, smart
            ModelTier::Cloud => 100.0, // Premium
        }
    }
}

impl fmt::Display for ModelTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelTier::Fast => write!(f, "fast"),
            ModelTier::Flash => write!(f, "flash"),
            ModelTier::Mini => write!(f, "mini"),
            ModelTier::Code => write!(f, "code"),
            ModelTier::Full => write!(f, "full"),
            ModelTier::Cloud => write!(f, "cloud"),
        }
    }
}

impl std::str::FromStr for ModelTier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "fast" | "8b" | "quick" => Ok(ModelTier::Fast),
            "flash" | "glm-flash" => Ok(ModelTier::Flash),
            "mini" | "ministral" | "14b" => Ok(ModelTier::Mini),
            "code" | "devstral" | "codegen" => Ok(ModelTier::Code),
            "full" | "glm" | "glm-4.7" => Ok(ModelTier::Full),
            "cloud" | "premium" | "opus" | "gpt-5" | "gpt-5.2" => Ok(ModelTier::Cloud),
            _ => Err(format!("Unknown model tier: {}", s)),
        }
    }
}

/// A specific model endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEndpoint {
    /// Model identifier.
    pub model: String,
    /// API endpoint URL.
    pub url: String,
    /// API key environment variable (if needed).
    pub api_key_env: Option<String>,
    /// Model tier.
    pub tier: ModelTier,
    /// Whether this model is currently available.
    pub available: bool,
}

impl ModelEndpoint {
    /// Create a local LM Studio endpoint.
    pub fn lm_studio(model: &str, tier: ModelTier) -> Self {
        Self {
            model: model.to_string(),
            url: "http://localhost:1234/v1".to_string(),
            api_key_env: None,
            tier,
            available: true,
        }
    }

    /// Create a remote endpoint.
    pub fn remote(model: &str, url: &str, api_key_env: &str, tier: ModelTier) -> Self {
        Self {
            model: model.to_string(),
            url: url.to_string(),
            api_key_env: Some(api_key_env.to_string()),
            tier,
            available: true,
        }
    }
}

/// Model ladder configuration.
#[derive(Debug, Clone)]
pub struct ModelLadder {
    /// Available model endpoints by tier.
    endpoints: Vec<ModelEndpoint>,
    /// Current tier.
    current_tier: ModelTier,
    /// Maximum tier allowed for this session.
    max_tier: ModelTier,
    /// Whether auto-upgrade is enabled.
    auto_upgrade: bool,
}

impl ModelLadder {
    /// Create a new model ladder with default endpoints.
    pub fn new() -> Self {
        Self {
            endpoints: Self::default_endpoints(),
            current_tier: ModelTier::Fast,
            max_tier: ModelTier::Cloud,
            auto_upgrade: true,
        }
    }

    /// Default model endpoints.
    fn default_endpoints() -> Vec<ModelEndpoint> {
        vec![
            // Local models (LM Studio)
            ModelEndpoint::lm_studio("nvidia_orchestrator-8b", ModelTier::Fast),
            ModelEndpoint::lm_studio("glm-4.7-flash", ModelTier::Flash),
            ModelEndpoint::lm_studio("ministral-3-14b-reasoning", ModelTier::Mini),

            // Remote code models (Mistral)
            ModelEndpoint::remote(
                "devstral-small-2505",
                "https://api.mistral.ai/v1",
                "MISTRAL_API_KEY",
                ModelTier::Code,
            ),

            // Full capability (Z.ai)
            ModelEndpoint::remote(
                "glm-4.7",
                "https://open.bigmodel.cn/api/paas/v4",
                "ZHIPU_API_KEY",
                ModelTier::Full,
            ),

            // Premium cloud (OpenRouter)
            ModelEndpoint::remote(
                "openai/gpt-5.2",
                "https://openrouter.ai/api/v1",
                "OPENROUTER_API_KEY",
                ModelTier::Cloud,
            ),
            ModelEndpoint::remote(
                "anthropic/claude-opus-4-5-20251101",
                "https://openrouter.ai/api/v1",
                "OPENROUTER_API_KEY",
                ModelTier::Cloud,
            ),
        ]
    }

    /// Start at a specific tier.
    pub fn starting_at(mut self, tier: ModelTier) -> Self {
        self.current_tier = tier;
        self
    }

    /// Set maximum tier.
    pub fn max_tier(mut self, tier: ModelTier) -> Self {
        self.max_tier = tier;
        self
    }

    /// Disable auto-upgrade.
    pub fn no_auto_upgrade(mut self) -> Self {
        self.auto_upgrade = false;
        self
    }

    /// Get current tier.
    pub fn current(&self) -> ModelTier {
        self.current_tier
    }

    /// Get the current model endpoint.
    pub fn current_endpoint(&self) -> Option<&ModelEndpoint> {
        self.endpoints.iter()
            .find(|e| e.tier == self.current_tier && e.available)
    }

    /// Get endpoint for a specific tier.
    pub fn endpoint_for(&self, tier: ModelTier) -> Option<&ModelEndpoint> {
        self.endpoints.iter()
            .find(|e| e.tier == tier && e.available)
    }

    /// Request an upgrade to the next tier.
    pub fn request_upgrade(&mut self, reason: &str) -> Result<ModelTier, String> {
        tracing::info!("Upgrade requested: {}", reason);

        if let Some(next) = self.current_tier.upgrade() {
            if next <= self.max_tier {
                self.current_tier = next;
                tracing::info!("Upgraded to tier: {}", next);
                Ok(next)
            } else {
                Err(format!("Cannot upgrade past max tier: {}", self.max_tier))
            }
        } else {
            Err("Already at maximum tier".to_string())
        }
    }

    /// Request upgrade to a specific tier.
    pub fn request_tier(&mut self, tier: ModelTier, reason: &str) -> Result<ModelTier, String> {
        tracing::info!("Tier {} requested: {}", tier, reason);

        if tier > self.max_tier {
            return Err(format!("Tier {} exceeds max allowed: {}", tier, self.max_tier));
        }

        if self.endpoint_for(tier).is_none() {
            return Err(format!("No available endpoint for tier: {}", tier));
        }

        self.current_tier = tier;
        Ok(tier)
    }

    /// Check if auto-upgrade is warranted based on complexity signals.
    pub fn should_auto_upgrade(&self, signals: &ComplexitySignals) -> Option<ModelTier> {
        if !self.auto_upgrade {
            return None;
        }

        // Determine required tier based on signals
        let required = signals.recommended_tier();

        if required > self.current_tier && required <= self.max_tier {
            Some(required)
        } else {
            None
        }
    }

    /// Add a custom endpoint.
    pub fn add_endpoint(&mut self, endpoint: ModelEndpoint) {
        self.endpoints.push(endpoint);
    }

    /// Mark an endpoint as unavailable.
    pub fn mark_unavailable(&mut self, model: &str) {
        for endpoint in &mut self.endpoints {
            if endpoint.model == model {
                endpoint.available = false;
            }
        }
    }
}

impl Default for ModelLadder {
    fn default() -> Self {
        Self::new()
    }
}

/// Signals that indicate task complexity.
#[derive(Debug, Clone, Default)]
pub struct ComplexitySignals {
    /// Number of files involved.
    pub file_count: usize,
    /// Whether it involves architecture changes.
    pub architectural: bool,
    /// Whether it requires multi-step reasoning.
    pub multi_step: bool,
    /// Estimated lines of code to change.
    pub loc_estimate: usize,
    /// Whether it involves breaking changes.
    pub breaking_changes: bool,
    /// Error recovery attempts.
    pub error_count: usize,
    /// Whether previous attempt failed.
    pub previous_failed: bool,
    /// Explicit upgrade request from agent.
    pub explicit_request: Option<ModelTier>,
}

impl ComplexitySignals {
    /// Determine recommended tier based on signals.
    pub fn recommended_tier(&self) -> ModelTier {
        // Explicit request takes priority
        if let Some(tier) = self.explicit_request {
            return tier;
        }

        // Previous failure suggests upgrade
        if self.previous_failed {
            if self.error_count >= 3 {
                return ModelTier::Full;
            }
            return ModelTier::Mini;
        }

        // Architectural changes need strong models
        if self.architectural || self.breaking_changes {
            return ModelTier::Full;
        }

        // Multi-file changes
        if self.file_count > 10 || self.loc_estimate > 500 {
            return ModelTier::Code;
        }

        if self.file_count > 3 || self.loc_estimate > 100 {
            return ModelTier::Mini;
        }

        // Multi-step reasoning
        if self.multi_step {
            return ModelTier::Flash;
        }

        // Default to fast
        ModelTier::Fast
    }
}

/// Upgrade request from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeRequest {
    /// Requested tier (or None for "just upgrade").
    pub tier: Option<ModelTier>,
    /// Reason for the upgrade.
    pub reason: String,
    /// Current task context.
    pub context: Option<String>,
}

impl UpgradeRequest {
    /// Create a simple upgrade request.
    pub fn upgrade(reason: impl Into<String>) -> Self {
        Self {
            tier: None,
            reason: reason.into(),
            context: None,
        }
    }

    /// Request a specific tier.
    pub fn to_tier(tier: ModelTier, reason: impl Into<String>) -> Self {
        Self {
            tier: Some(tier),
            reason: reason.into(),
            context: None,
        }
    }

    /// Add context.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_ordering() {
        assert!(ModelTier::Fast < ModelTier::Flash);
        assert!(ModelTier::Flash < ModelTier::Mini);
        assert!(ModelTier::Mini < ModelTier::Code);
        assert!(ModelTier::Code < ModelTier::Full);
        assert!(ModelTier::Full < ModelTier::Cloud);
    }

    #[test]
    fn test_tier_upgrade() {
        assert_eq!(ModelTier::Fast.upgrade(), Some(ModelTier::Flash));
        assert_eq!(ModelTier::Cloud.upgrade(), None);
    }

    #[test]
    fn test_ladder_upgrade() {
        let mut ladder = ModelLadder::new();
        assert_eq!(ladder.current(), ModelTier::Fast);

        ladder.request_upgrade("Need more capability").unwrap();
        assert_eq!(ladder.current(), ModelTier::Flash);

        ladder.request_tier(ModelTier::Full, "Complex task").unwrap();
        assert_eq!(ladder.current(), ModelTier::Full);
    }

    #[test]
    fn test_complexity_signals() {
        let signals = ComplexitySignals {
            file_count: 15,
            loc_estimate: 600,
            ..Default::default()
        };
        assert_eq!(signals.recommended_tier(), ModelTier::Code);

        let signals = ComplexitySignals {
            architectural: true,
            ..Default::default()
        };
        assert_eq!(signals.recommended_tier(), ModelTier::Full);
    }

    #[test]
    fn test_max_tier_limit() {
        let mut ladder = ModelLadder::new().max_tier(ModelTier::Mini);

        assert!(ladder.request_tier(ModelTier::Full, "test").is_err());
        assert!(ladder.request_tier(ModelTier::Mini, "test").is_ok());
    }
}
