//! LLM inference via LM Studio (OpenAI-compatible API).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// LM Studio client.
pub struct LMStudioClient {
    client: reqwest::Client,
    base_url: String,
    model: Option<String>,
}

impl LMStudioClient {
    /// Create a new LM Studio client.
    pub fn new(base_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: None,
        }
    }

    /// Test connection and get available models.
    pub async fn test_connection(&self) -> Result<Vec<ModelInfo>> {
        let url = format!("{}/models", self.base_url);
        debug!("Testing connection to: {}", url);

        let response = self.client
            .get(&url)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("LM Studio returned status: {}", response.status());
        }

        let models: ModelsResponse = response.json().await?;
        Ok(models.data)
    }

    /// Set the model to use.
    pub fn set_model(&mut self, model: &str) {
        self.model = Some(model.to_string());
    }

    /// Send a chat completion request.
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url);

        let model = self.model.as_deref().unwrap_or("local-model");

        let request = ChatRequest {
            model: model.to_string(),
            messages,
            temperature: 0.7,
            max_tokens: 1024,
            stream: false,
        };

        debug!("Sending chat request to: {}", url);

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("LM Studio returned status {}: {}", status, text);
        }

        let completion: ChatResponse = response.json().await?;

        completion.choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| anyhow::anyhow!("No response from model"))
    }

    /// Send a simple prompt and get a response.
    pub async fn prompt(&self, prompt: &str) -> Result<String> {
        self.chat(vec![ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        }]).await
    }

    /// Send a system + user prompt.
    pub async fn prompt_with_system(&self, system: &str, user: &str) -> Result<String> {
        self.chat(vec![
            ChatMessage {
                role: "system".to_string(),
                content: system.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user.to_string(),
            },
        ]).await
    }
}

/// Chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Chat completion request.
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
    stream: bool,
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

/// Models list response.
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

/// Model info.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub owned_by: String,
}

/// Test LM Studio connection.
pub async fn test_connection(base_url: &str, prompt: &str) -> Result<()> {
    println!("Testing connection to LM Studio at: {}", base_url);
    println!();

    let client = LMStudioClient::new(base_url);

    // Test connection and list models
    match client.test_connection().await {
        Ok(models) => {
            println!("Connected! Available models:");
            for model in &models {
                println!("  - {}", model.id);
            }
            println!();

            // Try to send a prompt
            println!("Sending prompt: \"{}\"", prompt);
            println!();

            match client.prompt(prompt).await {
                Ok(response) => {
                    println!("Response:");
                    println!("{}", response);
                }
                Err(e) => {
                    warn!("Failed to get response: {}", e);
                    println!("Failed to get response: {}", e);
                    println!();
                    println!("Make sure a model is loaded in LM Studio.");
                }
            }
        }
        Err(e) => {
            println!("Failed to connect: {}", e);
            println!();
            println!("Make sure LM Studio is running with the local server enabled.");
            println!("Default URL: http://localhost:1234/v1");
        }
    }

    Ok(())
}

/// Game state prompt builder for Pokemon.
pub struct PokemonPromptBuilder;

impl PokemonPromptBuilder {
    /// System prompt for Pokemon gameplay.
    pub fn system_prompt() -> &'static str {
        r#"You are an AI playing Pokemon Emerald on a GBA emulator. Your goal is to progress through the game efficiently.

You can see the current game screen and must decide what button to press next.

Available actions:
- press_a: Confirm, interact, advance text
- press_b: Cancel, run from battle
- press_up, press_down, press_left, press_right: Move or navigate menus
- press_start: Open menu
- press_select: Special functions
- press_l, press_r: Shoulder buttons
- wait: Do nothing this frame

Respond with a JSON object:
{
  "action": "press_a",
  "reasoning": "Brief explanation",
  "confidence": 0.8
}

Be decisive. If unsure, press A to advance or move toward objectives."#
    }

    /// Build a user prompt from game state.
    pub fn user_prompt(
        screen_base64: Option<&str>,
        badges: u8,
        location: &str,
        party_count: u8,
    ) -> String {
        let mut prompt = format!(
            "Current state:\n- Badges: {}\n- Location: {}\n- Party size: {}\n",
            badges, location, party_count
        );

        if let Some(screen) = screen_base64 {
            prompt.push_str(&format!("\nScreen (base64 PNG): {}\n", screen));
        }

        prompt.push_str("\nWhat action should I take?");
        prompt
    }
}
