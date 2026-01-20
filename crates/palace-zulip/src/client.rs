//! Zulip API client.

use crate::{ZulipError, ZulipResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

/// Zulip client configuration.
#[derive(Debug, Clone)]
pub struct ZulipConfig {
    /// Zulip server URL.
    pub server_url: Url,
    /// Bot email.
    pub email: String,
    /// API key.
    pub api_key: String,
    /// Skip SSL verification (for self-signed certs).
    pub insecure: bool,
}

impl Default for ZulipConfig {
    fn default() -> Self {
        Self {
            server_url: "http://localhost:8080".parse().unwrap(),
            email: "palace-bot@localhost".to_string(),
            api_key: String::new(),
            insecure: false,
        }
    }
}

impl ZulipConfig {
    /// Create config from environment variables.
    /// Reads: ZULIP_SERVER_URL, ZULIP_BOT_EMAIL, ZULIP_API_KEY, ZULIP_INSECURE
    pub fn from_env() -> ZulipResult<Self> {
        // Try to load .env file if present
        let _ = dotenvy::dotenv();

        let server_url = std::env::var("ZULIP_SERVER_URL")
            .unwrap_or_else(|_| "https://localhost:8443".to_string())
            .parse()
            .map_err(ZulipError::Url)?;

        let email = std::env::var("ZULIP_BOT_EMAIL")
            .map_err(|_| ZulipError::Auth("ZULIP_BOT_EMAIL not set".to_string()))?;

        let api_key = std::env::var("ZULIP_API_KEY")
            .map_err(|_| ZulipError::Auth("ZULIP_API_KEY not set".to_string()))?;

        let insecure = std::env::var("ZULIP_INSECURE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        Ok(Self {
            server_url,
            email,
            api_key,
            insecure,
        })
    }

    /// Create config for local development with self-signed cert.
    pub fn local(email: &str, api_key: &str) -> Self {
        Self {
            server_url: "https://localhost:8443".parse().unwrap(),
            email: email.to_string(),
            api_key: api_key.to_string(),
            insecure: true, // Allow self-signed certs locally
        }
    }
}

/// Zulip API client.
pub struct ZulipClient {
    config: ZulipConfig,
    http: Client,
}

impl ZulipClient {
    /// Create a new Zulip client.
    pub fn new(config: ZulipConfig) -> ZulipResult<Self> {
        let http = if config.insecure {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| ZulipError::Connection(e.to_string()))?
        } else {
            Client::new()
        };
        Ok(Self { config, http })
    }

    /// Validate connection by fetching server settings.
    pub async fn validate_connection(&self) -> ZulipResult<()> {
        let url = self.api_url("/server_settings")?;

        let resp = self.http.get(url)
            .send()
            .await
            .map_err(|e| ZulipError::Connection(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(ZulipError::Connection(format!(
                "Server returned status: {}",
                resp.status()
            )));
        }

        Ok(())
    }

    /// Send a message to a stream.
    pub async fn send_message(&self, stream: &str, topic: &str, content: &str) -> ZulipResult<u64> {
        let url = self.api_url("/messages")?;

        let resp = self.http.post(url)
            .basic_auth(&self.config.email, Some(&self.config.api_key))
            .form(&[
                ("type", "stream"),
                ("to", stream),
                ("topic", topic),
                ("content", content),
            ])
            .send()
            .await
            .map_err(|e| ZulipError::SendFailed(e.to_string()))?;

        let body: SendMessageResponse = resp.json().await
            .map_err(|e| ZulipError::SendFailed(e.to_string()))?;

        if body.result != "success" {
            return Err(ZulipError::SendFailed(body.msg.unwrap_or_default()));
        }

        Ok(body.id.unwrap_or(0))
    }

    /// Subscribe to a stream.
    pub async fn subscribe_to_stream(&self, stream: &str) -> ZulipResult<()> {
        let url = self.api_url("/users/me/subscriptions")?;

        let subscriptions = serde_json::json!([{"name": stream}]);

        let resp = self.http.post(url)
            .basic_auth(&self.config.email, Some(&self.config.api_key))
            .form(&[("subscriptions", subscriptions.to_string())])
            .send()
            .await
            .map_err(|e| ZulipError::Api(e.to_string()))?;

        let body: ApiResponse = resp.json().await
            .map_err(|e| ZulipError::Api(e.to_string()))?;

        if body.result != "success" {
            return Err(ZulipError::Api(body.msg.unwrap_or_default()));
        }

        Ok(())
    }

    /// Create a new stream.
    pub async fn create_stream(&self, name: &str, description: &str) -> ZulipResult<()> {
        let url = self.api_url("/users/me/subscriptions")?;

        let subscriptions = serde_json::json!([{
            "name": name,
            "description": description
        }]);

        let resp = self.http.post(url)
            .basic_auth(&self.config.email, Some(&self.config.api_key))
            .form(&[("subscriptions", subscriptions.to_string())])
            .send()
            .await
            .map_err(|e| ZulipError::Api(e.to_string()))?;

        let body: ApiResponse = resp.json().await
            .map_err(|e| ZulipError::Api(e.to_string()))?;

        if body.result != "success" {
            // Stream might already exist, that's OK
            if !body.msg.as_ref().map(|m| m.contains("already")).unwrap_or(false) {
                return Err(ZulipError::Api(body.msg.unwrap_or_default()));
            }
        }

        Ok(())
    }

    /// Get messages from a stream.
    pub async fn get_messages(
        &self,
        stream: &str,
        topic: Option<&str>,
        num_before: u32,
        num_after: u32,
    ) -> ZulipResult<Vec<crate::Message>> {
        let url = self.api_url("/messages")?;

        let mut narrow = vec![
            serde_json::json!({"operator": "stream", "operand": stream})
        ];
        if let Some(t) = topic {
            narrow.push(serde_json::json!({"operator": "topic", "operand": t}));
        }

        let resp = self.http.get(url)
            .basic_auth(&self.config.email, Some(&self.config.api_key))
            .query(&[
                ("anchor", "newest"),
                ("num_before", &num_before.to_string()),
                ("num_after", &num_after.to_string()),
                ("narrow", &serde_json::to_string(&narrow).unwrap()),
            ])
            .send()
            .await
            .map_err(|e| ZulipError::Api(e.to_string()))?;

        let body: GetMessagesResponse = resp.json().await
            .map_err(|e| ZulipError::Api(e.to_string()))?;

        if body.result != "success" {
            return Err(ZulipError::Api(body.msg.unwrap_or_default()));
        }

        Ok(body.messages.into_iter().map(|m| crate::Message {
            id: m.id,
            stream: m.stream_id.map(|id| id.to_string()),
            topic: Some(m.subject),
            content: m.content,
            sender: crate::Sender {
                id: m.sender_id,
                email: m.sender_email,
                name: m.sender_full_name,
            },
            timestamp: chrono::DateTime::from_timestamp(m.timestamp as i64, 0)
                .unwrap_or_default(),
            message_type: crate::MessageType::Stream,
        }).collect())
    }

    /// Register an event queue to receive real-time events.
    pub async fn register_event_queue(&self, event_types: &[&str]) -> ZulipResult<(String, i64)> {
        let url = self.api_url("/register")?;

        let event_types_json = serde_json::to_string(event_types)
            .map_err(|e| ZulipError::Api(e.to_string()))?;

        let resp = self.http.post(url)
            .basic_auth(&self.config.email, Some(&self.config.api_key))
            .form(&[
                ("event_types", event_types_json.as_str()),
            ])
            .send()
            .await
            .map_err(|e| ZulipError::Api(e.to_string()))?;

        let body: RegisterResponse = resp.json().await
            .map_err(|e| ZulipError::Api(e.to_string()))?;

        if body.result != "success" {
            return Err(ZulipError::Api(body.msg.unwrap_or_default()));
        }

        Ok((body.queue_id, body.last_event_id))
    }

    /// Get events from the queue (long-polling).
    pub async fn get_events(&self, queue_id: &str, last_event_id: i64) -> ZulipResult<Vec<ZulipEvent>> {
        let url = self.api_url("/events")?;

        let resp = self.http.get(url)
            .basic_auth(&self.config.email, Some(&self.config.api_key))
            .query(&[
                ("queue_id", queue_id),
                ("last_event_id", &last_event_id.to_string()),
            ])
            .send()
            .await
            .map_err(|e| ZulipError::Api(e.to_string()))?;

        let body: EventsResponse = resp.json().await
            .map_err(|e| ZulipError::Api(e.to_string()))?;

        if body.result != "success" {
            return Err(ZulipError::Api(body.msg.unwrap_or_default()));
        }

        Ok(body.events)
    }

    /// Build API URL.
    fn api_url(&self, path: &str) -> ZulipResult<Url> {
        let mut url = self.config.server_url.clone();
        url.set_path(&format!("/api/v1{}", path));
        Ok(url)
    }
}

#[derive(Debug, Deserialize)]
struct RegisterResponse {
    result: String,
    msg: Option<String>,
    queue_id: String,
    last_event_id: i64,
}

#[derive(Debug, Deserialize)]
struct EventsResponse {
    result: String,
    msg: Option<String>,
    #[serde(default)]
    events: Vec<ZulipEvent>,
}

/// A Zulip event from the events API.
#[derive(Debug, Clone, Deserialize)]
pub struct ZulipEvent {
    pub id: i64,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub message: Option<EventMessage>,
}

/// Message from an event.
#[derive(Debug, Clone, Deserialize)]
pub struct EventMessage {
    pub id: u64,
    pub sender_id: u64,
    pub sender_email: String,
    pub sender_full_name: String,
    #[serde(default)]
    pub display_recipient: serde_json::Value,
    pub subject: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    result: String,
    msg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SendMessageResponse {
    result: String,
    msg: Option<String>,
    id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GetMessagesResponse {
    result: String,
    msg: Option<String>,
    messages: Vec<ZulipMessage>,
}

#[derive(Debug, Deserialize)]
struct ZulipMessage {
    id: u64,
    sender_id: u64,
    sender_email: String,
    sender_full_name: String,
    stream_id: Option<u64>,
    subject: String,
    content: String,
    timestamp: u64,
}
