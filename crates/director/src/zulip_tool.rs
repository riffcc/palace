//! Zulip tool for Director - allows sending messages and commands via Zulip.

use crate::{DirectorError, DirectorResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Zulip tool for sending messages and interacting with Palace via chat.
#[derive(Clone)]
pub struct ZulipTool {
    client: Client,
    server_url: String,
    email: String,
    api_key: String,
}

impl ZulipTool {
    /// Create Director bot from credentials.
    pub fn from_env() -> DirectorResult<Self> {
        Self::from_env_director()
    }

    /// Create Director bot from credentials.
    pub fn from_env_director() -> DirectorResult<Self> {
        let _ = dotenvy::dotenv();
        let creds = palace_plane::Credentials::load()
            .map_err(|e| DirectorError::Config(e.to_string()))?;

        let server_url = creds.zulip_server_url()
            .unwrap_or_else(|| "https://localhost:8443".to_string());

        let email = creds.director_bot_email()
            .ok_or_else(|| DirectorError::Config("Director bot email not configured".into()))?;

        let api_key = creds.director_api_key()
            .ok_or_else(|| DirectorError::Config("Director bot API key not configured".into()))?;

        let insecure = creds.zulip_insecure();
        Self::build(server_url, email, api_key, insecure)
    }

    /// Create Palace bot from credentials.
    pub fn from_env_palace() -> DirectorResult<Self> {
        let _ = dotenvy::dotenv();
        let creds = palace_plane::Credentials::load()
            .map_err(|e| DirectorError::Config(e.to_string()))?;

        let server_url = creds.zulip_server_url()
            .unwrap_or_else(|| "https://localhost:8443".to_string());

        let email = creds.palace_bot_email()
            .ok_or_else(|| DirectorError::Config("Palace bot email not configured".into()))?;

        let api_key = creds.palace_api_key()
            .ok_or_else(|| DirectorError::Config("Palace bot API key not configured".into()))?;

        let insecure = creds.zulip_insecure();
        Self::build(server_url, email, api_key, insecure)
    }

    fn build(server_url: String, email: String, api_key: String, insecure: bool) -> DirectorResult<Self> {

        let client = if insecure {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| DirectorError::Config(e.to_string()))?
        } else {
            Client::new()
        };

        Ok(Self {
            client,
            server_url,
            email,
            api_key,
        })
    }

    /// Send a message to a stream/topic.
    pub async fn send(&self, stream: &str, topic: &str, content: &str) -> DirectorResult<u64> {
        self.send_with_widget(stream, topic, content, None).await
    }

    /// Send a message with optional widget content (polls, todo lists).
    pub async fn send_with_widget(
        &self,
        stream: &str,
        topic: &str,
        content: &str,
        widget_content: Option<&str>,
    ) -> DirectorResult<u64> {
        let url = format!("{}/api/v1/messages", self.server_url);

        let mut form = vec![
            ("type", "stream".to_string()),
            ("to", stream.to_string()),
            ("topic", topic.to_string()),
            ("content", content.to_string()),
        ];

        if let Some(widget) = widget_content {
            form.push(("widget_content", widget.to_string()));
        }

        let resp = self.client.post(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .form(&form)
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: ApiResponse = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        if body.result != "success" {
            return Err(DirectorError::Zulip(body.msg.unwrap_or_default()));
        }

        Ok(body.id.unwrap_or(0))
    }

    /// Send a native poll using /poll slash command syntax.
    /// Zulip parses this and creates the interactive widget.
    pub async fn send_poll(
        &self,
        stream: &str,
        topic: &str,
        question: &str,
        options: &[&str],
    ) -> DirectorResult<u64> {
        // Zulip /poll format: /poll Question?\nOption 1\nOption 2\n...
        let options_text = options.join("\n");
        let content = format!("/poll {}\n{}", question, options_text);
        self.send(stream, topic, &content).await
    }

    /// Send a native todo list using /todo slash command syntax.
    pub async fn send_todo(
        &self,
        stream: &str,
        topic: &str,
        title: &str,
        tasks: &[&str],
    ) -> DirectorResult<u64> {
        // Zulip /todo format: /todo Title\nTask 1\nTask 2\n...
        let tasks_text = tasks.join("\n");
        let content = format!("/todo {}\n{}", title, tasks_text);
        self.send(stream, topic, &content).await
    }

    /// Update a message (for live-editing todo lists).
    pub async fn update_message(&self, message_id: u64, content: &str) -> DirectorResult<()> {
        let url = format!("{}/api/v1/messages/{}", self.server_url, message_id);

        let resp = self.client.patch(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .form(&[("content", content)])
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: ApiResponse = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        if body.result != "success" {
            return Err(DirectorError::Zulip(body.msg.unwrap_or_default()));
        }

        Ok(())
    }

    /// Delete a message.
    pub async fn delete_message(&self, message_id: u64) -> DirectorResult<()> {
        let url = format!("{}/api/v1/messages/{}", self.server_url, message_id);

        let resp = self.client.delete(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: ApiResponse = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        if body.result != "success" {
            return Err(DirectorError::Zulip(body.msg.unwrap_or_default()));
        }

        Ok(())
    }

    /// Add a reaction to a message.
    pub async fn add_reaction(&self, message_id: u64, emoji: &str) -> DirectorResult<()> {
        let url = format!("{}/api/v1/messages/{}/reactions", self.server_url, message_id);

        let resp = self.client.post(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .form(&[("emoji_name", emoji)])
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: ApiResponse = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        // Ignore "already has reaction" errors
        if body.result != "success" && !body.msg.as_ref().map(|m| m.contains("already")).unwrap_or(false) {
            return Err(DirectorError::Zulip(body.msg.unwrap_or_default()));
        }

        Ok(())
    }

    /// Remove a reaction from a message.
    pub async fn remove_reaction(&self, message_id: u64, emoji: &str) -> DirectorResult<()> {
        let url = format!("{}/api/v1/messages/{}/reactions", self.server_url, message_id);

        let resp = self.client.delete(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .form(&[("emoji_name", emoji)])
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: ApiResponse = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        // Ignore "doesn't have reaction" errors
        if body.result != "success" && !body.msg.as_ref().map(|m| m.contains("doesn't exist")).unwrap_or(false) {
            return Err(DirectorError::Zulip(body.msg.unwrap_or_default()));
        }

        Ok(())
    }

    /// Send a command to Palace bot.
    pub async fn palace(&self, command: &str) -> DirectorResult<u64> {
        self.send("general", "commands", &format!("@**Palace** {}", command)).await
    }

    /// Get recent messages from a stream/topic.
    pub async fn get_messages(&self, stream: &str, topic: Option<&str>, limit: u32) -> DirectorResult<Vec<ZulipMessage>> {
        let url = format!("{}/api/v1/messages", self.server_url);

        let mut narrow = vec![
            serde_json::json!({"operator": "stream", "operand": stream})
        ];
        if let Some(t) = topic {
            narrow.push(serde_json::json!({"operator": "topic", "operand": t}));
        }

        let resp = self.client.get(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .query(&[
                ("anchor", "newest"),
                ("num_before", &limit.to_string()),
                ("num_after", "0"),
                ("narrow", &serde_json::to_string(&narrow).unwrap()),
            ])
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: GetMessagesResponse = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        if body.result != "success" {
            return Err(DirectorError::Zulip(body.msg.unwrap_or_default()));
        }

        Ok(body.messages)
    }

    /// Ensure a stream exists (creates if needed) and subscribe to it.
    pub async fn ensure_stream(&self, stream: &str) -> DirectorResult<()> {
        self.subscribe(stream).await
    }

    /// Subscribe to a stream (creates it if it doesn't exist).
    pub async fn subscribe(&self, stream: &str) -> DirectorResult<()> {
        let url = format!("{}/api/v1/users/me/subscriptions", self.server_url);

        let subscriptions = serde_json::json!([{"name": stream}]);

        let resp = self.client.post(&url)
            .basic_auth(&self.email, Some(&self.api_key))
            .form(&[("subscriptions", subscriptions.to_string())])
            .send()
            .await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        let body: ApiResponse = resp.json().await
            .map_err(|e| DirectorError::Zulip(e.to_string()))?;

        if body.result != "success" {
            return Err(DirectorError::Zulip(body.msg.unwrap_or_default()));
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    result: String,
    msg: Option<String>,
    id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GetMessagesResponse {
    result: String,
    msg: Option<String>,
    #[serde(default)]
    messages: Vec<ZulipMessage>,
}

/// A Zulip message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZulipMessage {
    pub id: u64,
    pub sender_id: u64,
    pub sender_email: String,
    pub sender_full_name: String,
    pub subject: String,
    pub content: String,
    pub timestamp: u64,
}

impl ZulipMessage {
    /// Strip HTML from content.
    pub fn text(&self) -> String {
        // Simple HTML stripping
        let re = regex::Regex::new(r"<[^>]+>").unwrap();
        let text = re.replace_all(&self.content, "");
        html_escape::decode_html_entities(&text).to_string()
    }
}
