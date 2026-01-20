//! State streaming and decision receiving.

use crate::cascade::ControlDecision;
use crate::state::StateSnapshot;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};

/// Emits state snapshots to the cascade.
pub struct StateEmitter {
    tx: broadcast::Sender<StateSnapshot>,
    snapshot_count: Arc<Mutex<u64>>,
}

impl StateEmitter {
    /// Create a new state emitter.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            snapshot_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Emit a state snapshot.
    pub async fn emit(&self, snapshot: StateSnapshot) -> bool {
        let mut count = self.snapshot_count.lock().await;
        *count += 1;

        // Returns number of receivers that got the message
        self.tx.send(snapshot).is_ok()
    }

    /// Subscribe to state updates.
    pub fn subscribe(&self) -> broadcast::Receiver<StateSnapshot> {
        self.tx.subscribe()
    }

    /// Get the number of snapshots emitted.
    pub async fn snapshot_count(&self) -> u64 {
        *self.snapshot_count.lock().await
    }
}

impl Default for StateEmitter {
    fn default() -> Self {
        Self::new(100)
    }
}

/// Receives decisions from the cascade.
pub struct DecisionReceiver {
    rx: mpsc::Receiver<DecisionEvent>,
    tx: mpsc::Sender<DecisionEvent>,
}

impl DecisionReceiver {
    /// Create a new decision receiver.
    pub fn new(capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel(capacity);
        Self { rx, tx }
    }

    /// Get a sender for publishing decisions.
    pub fn sender(&self) -> mpsc::Sender<DecisionEvent> {
        self.tx.clone()
    }

    /// Receive the next decision.
    pub async fn recv(&mut self) -> Option<DecisionEvent> {
        self.rx.recv().await
    }

    /// Try to receive without blocking.
    pub fn try_recv(&mut self) -> Option<DecisionEvent> {
        self.rx.try_recv().ok()
    }
}

impl Default for DecisionReceiver {
    fn default() -> Self {
        Self::new(100)
    }
}

/// A decision event with timing information.
#[derive(Debug, Clone)]
pub struct DecisionEvent {
    /// The decision.
    pub decision: ControlDecision,

    /// Timestamp when decision was made (ms since epoch).
    pub decision_timestamp_ms: u64,

    /// Target execution timestamp (ms since epoch).
    pub target_timestamp_ms: u64,

    /// Model tier that made the final decision.
    pub source_tier: String,

    /// Confidence of the decision.
    pub confidence: f32,
}

impl DecisionEvent {
    /// Create a new decision event.
    pub fn new(decision: ControlDecision, source_tier: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            decision,
            decision_timestamp_ms: now,
            target_timestamp_ms: now,
            source_tier: source_tier.into(),
            confidence: 1.0,
        }
    }

    /// Set the target timestamp.
    pub fn with_target(mut self, target_ms: u64) -> Self {
        self.target_timestamp_ms = target_ms;
        self
    }

    /// Set confidence.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    /// Check if this decision is for a past execution point.
    pub fn is_past(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.target_timestamp_ms < now
    }

    /// Get time until this decision should be applied.
    pub fn time_until_target(&self) -> std::time::Duration {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if self.target_timestamp_ms > now {
            std::time::Duration::from_millis(self.target_timestamp_ms - now)
        } else {
            std::time::Duration::ZERO
        }
    }
}

/// Type of LLM output for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LLMOutputType {
    /// Thinking/reasoning text.
    Thinking,
    /// Tool call being made.
    ToolCall,
    /// Tool result received.
    ToolResult,
    /// Regular text output.
    Text,
    /// Decision made.
    Decision,
    /// Error occurred.
    Error,
}

/// A piece of LLM output for display.
#[derive(Debug, Clone)]
pub struct LLMOutput {
    /// Agent/model name that produced this.
    pub agent_name: String,
    /// The output text.
    pub text: String,
    /// Type of output.
    pub output_type: LLMOutputType,
    /// Timestamp when produced.
    pub timestamp_ms: u64,
}

impl LLMOutput {
    /// Create a new LLM output.
    pub fn new(agent: impl Into<String>, text: impl Into<String>, output_type: LLMOutputType) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            agent_name: agent.into(),
            text: text.into(),
            output_type,
            timestamp_ms: now,
        }
    }

    /// Create a thinking output.
    pub fn thinking(agent: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(agent, text, LLMOutputType::Thinking)
    }

    /// Create a tool call output.
    pub fn tool_call(agent: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(agent, text, LLMOutputType::ToolCall)
    }

    /// Create a tool result output.
    pub fn tool_result(agent: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(agent, text, LLMOutputType::ToolResult)
    }

    /// Create a text output.
    pub fn text(agent: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(agent, text, LLMOutputType::Text)
    }

    /// Create a decision output.
    pub fn decision(agent: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(agent, text, LLMOutputType::Decision)
    }

    /// Create an error output.
    pub fn error(agent: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(agent, text, LLMOutputType::Error)
    }
}

/// Stream of LLM outputs for UI display.
pub struct LLMOutputStream {
    tx: broadcast::Sender<LLMOutput>,
}

impl LLMOutputStream {
    /// Create a new LLM output stream.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Emit an output.
    pub fn emit(&self, output: LLMOutput) {
        let _ = self.tx.send(output);
    }

    /// Subscribe to outputs.
    pub fn subscribe(&self) -> broadcast::Receiver<LLMOutput> {
        self.tx.subscribe()
    }
}

impl Default for LLMOutputStream {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_state_emitter() {
        let emitter = StateEmitter::new(10);
        let mut subscriber = emitter.subscribe();

        let snapshot = StateSnapshot::default();
        emitter.emit(snapshot.clone()).await;

        let received = subscriber.recv().await.unwrap();
        assert_eq!(received.timestamp_ms, snapshot.timestamp_ms);
    }

    #[test]
    fn test_decision_event() {
        let decision = ControlDecision::default();
        let event = DecisionEvent::new(decision, "test-tier").with_confidence(0.9);

        assert_eq!(event.source_tier, "test-tier");
        assert_eq!(event.confidence, 0.9);
    }
}
