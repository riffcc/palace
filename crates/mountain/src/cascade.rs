//! Cascading model orchestration.
//!
//! The cascade runs multiple models in parallel, with faster models informing
//! slower models, and frozen models waiting for all prior results.

use crate::error::{MountainError, MountainResult};
use crate::model::ModelTier;
use crate::state::StateSnapshot;
use crate::stream::{LLMOutput, LLMOutputStream};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, info, warn};

/// Response from a single model in the cascade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResponse {
    /// Model tier name.
    pub tier_name: String,

    /// The decision/action recommended.
    pub decision: ControlDecision,

    /// Confidence score (0.0 - 1.0).
    pub confidence: f32,

    /// Reasoning/explanation.
    pub reasoning: Option<String>,

    /// Response latency in milliseconds.
    pub latency_ms: u64,

    /// Whether this model received prior tier context.
    pub had_prior_context: bool,
}

/// A control decision from a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlDecision {
    /// The action to take.
    pub action: String,

    /// Parameters for the action.
    pub params: HashMap<String, serde_json::Value>,

    /// Whether to halt/pause execution.
    pub halt: bool,

    /// Optional override for specific state values.
    pub state_overrides: HashMap<String, serde_json::Value>,
}

impl Default for ControlDecision {
    fn default() -> Self {
        Self {
            action: "continue".into(),
            params: HashMap::new(),
            halt: false,
            state_overrides: HashMap::new(),
        }
    }
}

/// Result of running the full cascade.
#[derive(Debug, Clone)]
pub struct CascadeResult {
    /// All model responses, in order of completion.
    pub responses: Vec<ModelResponse>,

    /// The merged final decision.
    pub final_decision: ControlDecision,

    /// Total cascade duration.
    pub total_duration: Duration,

    /// Whether any model vetoed.
    pub vetoed: bool,

    /// Veto reason if vetoed.
    pub veto_reason: Option<String>,
}

/// OpenAI-compatible chat request.
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

/// Response format specification.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ResponseFormat {
    /// Simple type (json_object for OpenAI-compatible APIs)
    Simple {
        #[serde(rename = "type")]
        format_type: String,
    },
    /// JSON Schema format (for LM Studio)
    JsonSchema {
        #[serde(rename = "type")]
        format_type: String,
        json_schema: JsonSchemaSpec,
    },
}

/// JSON Schema specification for structured output.
#[derive(Debug, Serialize)]
struct JsonSchemaSpec {
    name: String,
    schema: serde_json::Value,
}

/// Chat message with support for multimodal content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum ChatContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl ChatContent {
    /// Extract text content (for parsing LLM responses).
    fn as_text(&self) -> String {
        match self {
            ChatContent::Text(s) => s.clone(),
            ChatContent::Parts(parts) => {
                parts.iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        }
    }
}

impl Default for ChatContent {
    fn default() -> Self {
        ChatContent::Text(String::new())
    }
}

/// Content part for multimodal messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

/// Image URL for vision input.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ImageUrl {
    url: String,
}

/// Chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: ChatContent,
}

/// Chat completion response.
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

/// Chat choice.
#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

/// Orchestrates the cascading model inference.
pub struct Cascade {
    tiers: Vec<ModelTier>,
    responses: Arc<RwLock<Vec<ModelResponse>>>,
    http_client: reqwest::Client,
    output_stream: Option<Arc<LLMOutputStream>>,
}

impl Cascade {
    /// Create a new cascade with the given tiers.
    /// Tiers should be sorted by expected latency.
    pub fn new(tiers: Vec<ModelTier>) -> Self {
        Self {
            tiers,
            responses: Arc::new(RwLock::new(vec![])),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
            output_stream: None,
        }
    }

    /// Set the output stream for LLM output display.
    pub fn with_output_stream(mut self, stream: Arc<LLMOutputStream>) -> Self {
        self.output_stream = Some(stream);
        self
    }

    /// Get the tiers.
    pub fn tiers(&self) -> &[ModelTier] {
        &self.tiers
    }

    /// Get output stream reference.
    pub fn output_stream(&self) -> Option<&Arc<LLMOutputStream>> {
        self.output_stream.as_ref()
    }

    /// Run the cascade for a given state snapshot.
    pub async fn run(&self, state: &StateSnapshot) -> MountainResult<CascadeResult> {
        let start = Instant::now();
        let (tx, mut rx) = mpsc::channel::<ModelResponse>(self.tiers.len().max(1));

        // Clear previous responses
        {
            let mut prev = self.responses.write().await;
            prev.clear();
        }

        // For non-frozen tiers, run sequentially to avoid overwhelming the API
        // (parallel mode can be enabled later for local models)
        let mut collected: Vec<ModelResponse> = vec![];

        for tier in &self.tiers {
            if !tier.frozen {
                match Self::run_tier_inference(
                    &self.http_client,
                    tier,
                    state,
                    None,
                    self.output_stream.as_ref(),
                ).await {
                    Ok(response) => {
                        collected.push(response);
                    }
                    Err(e) => {
                        warn!("Tier {} inference failed: {}", tier.name, e);
                    }
                }
            }
        }

        // Now run frozen tiers with prior context
        for tier in &self.tiers {
            if tier.frozen {
                let prior_context = Self::build_prior_context(&collected);
                match Self::run_tier_inference(
                    &self.http_client,
                    tier,
                    state,
                    Some(&prior_context),
                    self.output_stream.as_ref(),
                ).await {
                    Ok(response) => {
                        collected.push(response);
                    }
                    Err(e) => {
                        warn!("Frozen tier {} inference failed: {}", tier.name, e);
                    }
                }
            }
        }

        // Merge decisions
        let (final_decision, vetoed, veto_reason) = self.merge_decisions(&collected)?;

        // Store responses
        {
            let mut prev = self.responses.write().await;
            *prev = collected.clone();
        }

        Ok(CascadeResult {
            responses: collected,
            final_decision,
            total_duration: start.elapsed(),
            vetoed,
            veto_reason,
        })
    }

    /// Run inference for a single tier.
    async fn run_tier_inference(
        http_client: &reqwest::Client,
        tier: &ModelTier,
        state: &StateSnapshot,
        prior_context: Option<&str>,
        output_stream: Option<&Arc<LLMOutputStream>>,
    ) -> MountainResult<ModelResponse> {
        let start = Instant::now();

        // Build prompt
        let prompt = Self::build_prompt(tier, state, prior_context);

        // Get endpoint (default to LM Studio)
        let endpoint = tier.config.endpoint
            .as_deref()
            .unwrap_or("http://localhost:1234/v1");

        let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));

        // Emit thinking event
        if let Some(stream) = output_stream {
            stream.emit(LLMOutput::thinking(
                &tier.name,
                format!("Analyzing state (frame {})...", state.timestamp_ms),
            ));
        }

        // Build chat messages
        let mut messages = vec![];

        if let Some(ref system) = tier.config.system_prompt {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: ChatContent::Text(system.clone()),
            });
        } else {
            // Default system prompt for game control
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: ChatContent::Text(Self::default_system_prompt().to_string()),
            });
        }

        // Build user message with vision if screen data is available
        // Note: Only some models support vision (check endpoint/model)
        let screen_b64 = state.variables.get("screen_thumbnail_b64")
            .and_then(|v| v.as_str());

        // Z.ai glm-4.7-flash doesn't support vision, only glm-4v does
        let supports_vision = endpoint.contains("localhost")
            || endpoint.contains("openrouter")
            || tier.config.model_id.contains("4v")
            || tier.config.model_id.contains("vision");

        let user_content = if let Some(screen) = screen_b64 {
            if supports_vision {
                // Multimodal message with image
                ChatContent::Parts(vec![
                    ContentPart::ImageUrl {
                        image_url: ImageUrl {
                            url: format!("data:image/png;base64,{}", screen),
                        },
                    },
                    ContentPart::Text {
                        text: prompt,
                    },
                ])
            } else {
                // Text only for non-vision models
                ChatContent::Text(prompt)
            }
        } else {
            ChatContent::Text(prompt)
        };

        messages.push(ChatMessage {
            role: "user".to_string(),
            content: user_content,
        });

        // Use appropriate response_format for each API
        // LM Studio wants "json_schema" with full schema, OpenAI/OpenRouter want "json_object"
        let response_format = if endpoint.contains("localhost") {
            Some(ResponseFormat::JsonSchema {
                format_type: "json_schema".to_string(),
                json_schema: JsonSchemaSpec {
                    name: "game_decision".to_string(),
                    schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "enum": ["press", "release", "tap", "wait"]
                            },
                            "button": {
                                "type": "string",
                                "enum": ["a", "b", "up", "down", "left", "right", "start", "select", "l", "r"]
                            },
                            "reasoning": {
                                "type": "string"
                            },
                            "confidence": {
                                "type": "number"
                            }
                        },
                        "required": ["action", "confidence"]
                    }),
                },
            })
        } else if endpoint.contains("openrouter") || endpoint.contains("z.ai") {
            Some(ResponseFormat::Simple {
                format_type: "json_object".to_string(),
            })
        } else {
            None
        };

        let request = ChatRequest {
            model: tier.config.model_id.clone(),
            messages,
            temperature: tier.config.temperature,
            max_tokens: tier.config.max_tokens,
            response_format,
        };

        info!("Sending inference request to {} for tier {} (model: {})", url, tier.name, tier.config.model_id);
        if endpoint.contains("z.ai") {
            info!("Z.ai request body: {}", serde_json::to_string(&request).unwrap_or_default());
        }

        // Build request with appropriate auth headers
        let mut req = http_client
            .post(&url)
            .header("Content-Type", "application/json");

        // Add Authorization header for APIs that need it
        if endpoint.contains("z.ai") {
            if let Ok(api_key) = std::env::var("ZAI_API_KEY") {
                req = req.header("Authorization", format!("Bearer {}", api_key));
            }
        } else if endpoint.contains("openrouter") {
            if let Ok(api_key) = std::env::var("OPENROUTER_API_KEY") {
                req = req.header("Authorization", format!("Bearer {}", api_key));
            }
        }

        // Make the request
        let response = req
            .json(&request)
            .send()
            .await
            .map_err(|e| MountainError::Inference {
                model: tier.name.clone(),
                error: format!("HTTP request failed: {}", e),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();

            warn!("API error from {}: {} - {}", tier.name, status, text);

            return Err(MountainError::Inference {
                model: tier.name.clone(),
                error: format!("API returned {}: {}", status, text),
            });
        }

        let chat_response: ChatResponse = response.json().await.map_err(|e| {
            MountainError::Inference {
                model: tier.name.clone(),
                error: format!("Failed to parse response: {}", e),
            }
        })?;

        let content = chat_response
            .choices
            .first()
            .map(|c| c.message.content.as_text())
            .unwrap_or_default();

        info!("Got response from {}: {}", tier.name, content);

        // Parse the JSON response
        let (decision, confidence, reasoning) = Self::parse_decision_response(&content, &tier.name);

        // Emit decision event
        if let Some(stream) = output_stream {
            stream.emit(LLMOutput::decision(
                &tier.name,
                format!("{} (conf: {:.0}%)", decision.action, confidence * 100.0),
            ));
            if let Some(ref reason) = reasoning {
                stream.emit(LLMOutput::thinking(&tier.name, reason.clone()));
            }
        }

        Ok(ModelResponse {
            tier_name: tier.name.clone(),
            decision,
            confidence,
            reasoning,
            latency_ms: start.elapsed().as_millis() as u64,
            had_prior_context: prior_context.is_some(),
        })
    }

    /// Default system prompt for game control.
    fn default_system_prompt() -> &'static str {
        r#"You are an AI controlling a GBA game. Analyze the game state and decide what button to press.

Respond with a JSON object:
{
  "action": "press" | "release" | "tap" | "wait",
  "button": "a" | "b" | "up" | "down" | "left" | "right" | "start" | "select" | "l" | "r",
  "reasoning": "Brief explanation",
  "confidence": 0.0-1.0
}

For "wait" action, no button is needed. Be decisive and progress through the game."#
    }

    /// Parse a decision response from the model.
    fn parse_decision_response(content: &str, tier_name: &str) -> (ControlDecision, f32, Option<String>) {
        // Try to parse as JSON
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
            let action = json.get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("wait")
                .to_lowercase();

            let button = json.get("button")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();

            let confidence = json.get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.5) as f32;

            let reasoning = json.get("reasoning")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let mut params = HashMap::new();
            if !button.is_empty() {
                params.insert("button".to_string(), serde_json::json!(button));
            }

            return (
                ControlDecision {
                    action,
                    params,
                    halt: false,
                    state_overrides: HashMap::new(),
                },
                confidence,
                reasoning,
            );
        }

        // Fallback: try to extract action from text
        let content_lower = content.to_lowercase();
        let action = if content_lower.contains("press_a") || content_lower.contains("press a") {
            "tap"
        } else if content_lower.contains("press_b") || content_lower.contains("press b") {
            "tap"
        } else if content_lower.contains("up") {
            "tap"
        } else if content_lower.contains("down") {
            "tap"
        } else if content_lower.contains("left") {
            "tap"
        } else if content_lower.contains("right") {
            "tap"
        } else {
            "wait"
        };

        let button = if content_lower.contains("_a") || content_lower.contains(" a") {
            "a"
        } else if content_lower.contains("_b") || content_lower.contains(" b") {
            "b"
        } else if content_lower.contains("up") {
            "up"
        } else if content_lower.contains("down") {
            "down"
        } else if content_lower.contains("left") {
            "left"
        } else if content_lower.contains("right") {
            "right"
        } else {
            ""
        };

        let mut params = HashMap::new();
        if !button.is_empty() {
            params.insert("button".to_string(), serde_json::json!(button));
        }

        warn!("Failed to parse JSON from {}, extracted: action={}, button={}", tier_name, action, button);

        (
            ControlDecision {
                action: action.to_string(),
                params,
                halt: false,
                state_overrides: HashMap::new(),
            },
            0.5,
            Some(format!("Fallback parse from: {}", content.chars().take(100).collect::<String>())),
        )
    }

    /// Build the prompt for a tier.
    fn build_prompt(tier: &ModelTier, state: &StateSnapshot, prior_context: Option<&str>) -> String {
        let mut prompt = String::new();

        // System prompt if configured
        if let Some(ref sys) = tier.config.system_prompt {
            prompt.push_str(sys);
            prompt.push_str("\n\n");
        }

        // State context
        prompt.push_str("Current program state:\n");
        prompt.push_str(&serde_json::to_string_pretty(state).unwrap_or_default());
        prompt.push_str("\n\n");

        // Prior model context if available
        if let Some(context) = prior_context {
            prompt.push_str("Analysis from faster models:\n");
            prompt.push_str(context);
            prompt.push_str("\n\n");
        }

        prompt.push_str("What action should be taken? Respond with a JSON decision.");

        prompt
    }

    /// Build context string from prior responses.
    fn build_prior_context(responses: &[ModelResponse]) -> String {
        let mut context = String::new();
        for resp in responses {
            context.push_str(&format!("## {} (confidence: {:.2})\n", resp.tier_name, resp.confidence));
            if let Some(ref reasoning) = resp.reasoning {
                context.push_str(reasoning);
            }
            context.push_str(&format!("\nDecision: {}\n\n", resp.decision.action));
        }
        context
    }

    /// Merge decisions from all tiers.
    fn merge_decisions(
        &self,
        responses: &[ModelResponse],
    ) -> MountainResult<(ControlDecision, bool, Option<String>)> {
        if responses.is_empty() {
            return Ok((ControlDecision::default(), false, None));
        }

        // Check for vetoes
        for response in responses {
            let tier = self.tiers.iter().find(|t| t.name == response.tier_name);
            if let Some(tier) = tier {
                if tier.can_veto && response.decision.halt {
                    return Ok((
                        response.decision.clone(),
                        true,
                        response.reasoning.clone(),
                    ));
                }
            }
        }

        // Weighted merge based on priority and confidence
        let mut total_weight = 0.0f32;
        let mut weighted_decisions: HashMap<String, f32> = HashMap::new();

        for response in responses {
            let tier = self.tiers.iter().find(|t| t.name == response.tier_name);
            let priority = tier.map(|t| t.priority).unwrap_or(1) as f32;
            let weight = priority * response.confidence;
            total_weight += weight;

            *weighted_decisions
                .entry(response.decision.action.clone())
                .or_default() += weight;
        }

        // Pick highest weighted action
        let best_action = weighted_decisions
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(action, _)| action)
            .unwrap_or_else(|| "continue".into());

        // Use the decision from the highest-priority model that chose this action
        let best_response = responses
            .iter()
            .filter(|r| r.decision.action == best_action)
            .max_by_key(|r| {
                self.tiers
                    .iter()
                    .find(|t| t.name == r.tier_name)
                    .map(|t| t.priority)
                    .unwrap_or(0)
            });

        let final_decision = best_response
            .map(|r| r.decision.clone())
            .unwrap_or_default();

        Ok((final_decision, false, None))
    }
}

/// Builder for cascade configuration.
pub struct CascadeBuilder {
    tiers: Vec<ModelTier>,
    output_stream: Option<Arc<LLMOutputStream>>,
}

impl CascadeBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            tiers: vec![],
            output_stream: None,
        }
    }

    /// Add a tier.
    pub fn add_tier(mut self, tier: ModelTier) -> Self {
        self.tiers.push(tier);
        self
    }

    /// Set the output stream for LLM output display.
    pub fn with_output_stream(mut self, stream: Arc<LLMOutputStream>) -> Self {
        self.output_stream = Some(stream);
        self
    }

    /// Build the cascade.
    pub fn build(mut self) -> Cascade {
        // Sort by latency
        self.tiers.sort_by_key(|t| t.expected_latency_ms);
        let mut cascade = Cascade::new(self.tiers);
        if let Some(stream) = self.output_stream {
            cascade = cascade.with_output_stream(stream);
        }
        cascade
    }
}

impl Default for CascadeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cascade_builder() {
        let cascade = CascadeBuilder::new()
            .add_tier(ModelTier::local("small", 100))
            .add_tier(ModelTier::cloud("big", 2000).frozen())
            .add_tier(ModelTier::local("medium", 500))
            .build();

        // Should be sorted by latency
        assert_eq!(cascade.tiers[0].name, "small");
        assert_eq!(cascade.tiers[1].name, "medium");
        assert_eq!(cascade.tiers[2].name, "big");
    }

    #[test]
    fn test_decision_merge() {
        let cascade = CascadeBuilder::new()
            .add_tier(ModelTier::local("low", 100).with_priority(1))
            .add_tier(ModelTier::cloud("high", 500).with_priority(10))
            .build();

        let responses = vec![
            ModelResponse {
                tier_name: "low".into(),
                decision: ControlDecision {
                    action: "jump".into(),
                    ..Default::default()
                },
                confidence: 0.9,
                reasoning: None,
                latency_ms: 100,
                had_prior_context: false,
            },
            ModelResponse {
                tier_name: "high".into(),
                decision: ControlDecision {
                    action: "duck".into(),
                    ..Default::default()
                },
                confidence: 0.8,
                reasoning: None,
                latency_ms: 500,
                had_prior_context: true,
            },
        ];

        let (decision, vetoed, _) = cascade.merge_decisions(&responses).unwrap();
        // High priority model should win despite lower confidence
        assert_eq!(decision.action, "duck");
        assert!(!vetoed);
    }
}
