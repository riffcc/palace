//! Time-delayed execution buffer.
//!
//! The delay buffer holds program state and allows execution to run
//! "behind" realtime, giving LLMs time to process and respond before
//! their decisions are needed.

use crate::error::{MountainError, MountainResult};
use crate::state::StateSnapshot;
use crate::stream::DecisionEvent;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::sleep;

/// Configuration for the delay buffer.
#[derive(Debug, Clone)]
pub struct DelayConfig {
    /// Base delay in milliseconds.
    pub delay_ms: u64,

    /// Maximum buffer size (number of snapshots).
    pub max_buffer_size: usize,

    /// Whether to allow dynamic delay adjustment.
    pub dynamic_delay: bool,

    /// Minimum delay when using dynamic adjustment.
    pub min_delay_ms: u64,

    /// Maximum delay when using dynamic adjustment.
    pub max_delay_ms: u64,
}

impl Default for DelayConfig {
    fn default() -> Self {
        Self {
            delay_ms: 2000,        // 2 second default delay
            max_buffer_size: 1000,
            dynamic_delay: true,
            min_delay_ms: 500,
            max_delay_ms: 5000,
        }
    }
}

/// A buffered state snapshot with timing information.
#[derive(Debug, Clone)]
pub struct BufferedState {
    /// The state snapshot.
    pub snapshot: StateSnapshot,

    /// When this snapshot was received.
    pub received_at: Instant,

    /// When this snapshot should be released for execution.
    pub release_at: Instant,

    /// Associated decision if one has been made.
    pub decision: Option<DecisionEvent>,
}

impl BufferedState {
    /// Create a new buffered state.
    pub fn new(snapshot: StateSnapshot, delay: Duration) -> Self {
        let now = Instant::now();
        Self {
            snapshot,
            received_at: now,
            release_at: now + delay,
            decision: None,
        }
    }

    /// Check if this state is ready for release.
    pub fn is_ready(&self) -> bool {
        Instant::now() >= self.release_at
    }

    /// Time until release.
    pub fn time_until_release(&self) -> Duration {
        let now = Instant::now();
        if now >= self.release_at {
            Duration::ZERO
        } else {
            self.release_at - now
        }
    }

    /// Attach a decision to this state.
    pub fn with_decision(mut self, decision: DecisionEvent) -> Self {
        self.decision = Some(decision);
        self
    }
}

/// The delay buffer manages the time gap between realtime and execution.
pub struct DelayBuffer {
    config: DelayConfig,
    buffer: Arc<Mutex<VecDeque<BufferedState>>>,
    current_delay: Arc<RwLock<Duration>>,
    pending_decisions: Arc<Mutex<Vec<DecisionEvent>>>,
}

impl DelayBuffer {
    /// Create a new delay buffer.
    pub fn new(config: DelayConfig) -> Self {
        let initial_delay = Duration::from_millis(config.delay_ms);
        Self {
            config,
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            current_delay: Arc::new(RwLock::new(initial_delay)),
            pending_decisions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get the current delay.
    pub async fn current_delay(&self) -> Duration {
        *self.current_delay.read().await
    }

    /// Push a new state snapshot into the buffer.
    pub async fn push(&self, snapshot: StateSnapshot) -> MountainResult<()> {
        let delay = self.current_delay().await;
        let buffered = BufferedState::new(snapshot, delay);

        let mut buffer = self.buffer.lock().await;

        // Trim if over capacity
        while buffer.len() >= self.config.max_buffer_size {
            buffer.pop_front();
        }

        buffer.push_back(buffered);
        Ok(())
    }

    /// Add a decision that should be applied to buffered states.
    pub async fn add_decision(&self, decision: DecisionEvent) {
        let mut pending = self.pending_decisions.lock().await;
        pending.push(decision);
    }

    /// Pop the next ready state (if any).
    pub async fn pop_ready(&self) -> Option<BufferedState> {
        let mut buffer = self.buffer.lock().await;

        if let Some(front) = buffer.front() {
            if front.is_ready() {
                let mut state = buffer.pop_front()?;

                // Attach any relevant pending decision
                let mut pending = self.pending_decisions.lock().await;
                if let Some(idx) = pending.iter().position(|d| {
                    d.target_timestamp_ms <= state.snapshot.timestamp_ms
                }) {
                    state.decision = Some(pending.remove(idx));
                }

                return Some(state);
            }
        }

        None
    }

    /// Wait for and return the next ready state.
    pub async fn wait_next(&self) -> MountainResult<BufferedState> {
        loop {
            // Check if anything is ready
            if let Some(state) = self.pop_ready().await {
                return Ok(state);
            }

            // Check time until next release
            let buffer = self.buffer.lock().await;
            if let Some(front) = buffer.front() {
                let wait_time = front.time_until_release();
                drop(buffer);

                if wait_time > Duration::ZERO {
                    sleep(wait_time.min(Duration::from_millis(100))).await;
                }
            } else {
                drop(buffer);
                // Buffer empty, wait a bit
                sleep(Duration::from_millis(10)).await;
            }
        }
    }

    /// Peek at the next state without removing it.
    pub async fn peek(&self) -> Option<BufferedState> {
        let buffer = self.buffer.lock().await;
        buffer.front().cloned()
    }

    /// Get buffer statistics.
    pub async fn stats(&self) -> BufferStats {
        let buffer = self.buffer.lock().await;
        let pending = self.pending_decisions.lock().await;

        let ready_count = buffer.iter().filter(|s| s.is_ready()).count();
        let oldest_age = buffer.front().map(|s| s.received_at.elapsed());

        BufferStats {
            total_buffered: buffer.len(),
            ready_count,
            pending_decisions: pending.len(),
            oldest_age,
            current_delay: *self.current_delay.blocking_read(),
        }
    }

    /// Adjust the delay dynamically based on cascade performance.
    pub async fn adjust_delay(&self, cascade_latency: Duration) -> MountainResult<()> {
        if !self.config.dynamic_delay {
            return Ok(());
        }

        let mut current = self.current_delay.write().await;

        // Target: cascade completes with 20% buffer time
        let target_delay = cascade_latency + (cascade_latency / 5);

        // Clamp to configured bounds
        let target_ms = target_delay.as_millis() as u64;
        let clamped_ms = target_ms
            .max(self.config.min_delay_ms)
            .min(self.config.max_delay_ms);

        // Smooth adjustment (move 10% toward target)
        let current_ms = current.as_millis() as u64;
        let new_ms = current_ms + ((clamped_ms as i64 - current_ms as i64) / 10) as u64;

        *current = Duration::from_millis(new_ms);
        Ok(())
    }

    /// Clear the buffer.
    pub async fn clear(&self) {
        let mut buffer = self.buffer.lock().await;
        buffer.clear();
    }

    /// Check if buffer is empty.
    pub async fn is_empty(&self) -> bool {
        let buffer = self.buffer.lock().await;
        buffer.is_empty()
    }

    /// Get buffer length.
    pub async fn len(&self) -> usize {
        let buffer = self.buffer.lock().await;
        buffer.len()
    }
}

impl Default for DelayBuffer {
    fn default() -> Self {
        Self::new(DelayConfig::default())
    }
}

/// Statistics about the buffer state.
#[derive(Debug, Clone)]
pub struct BufferStats {
    /// Total snapshots in buffer.
    pub total_buffered: usize,

    /// Snapshots ready for release.
    pub ready_count: usize,

    /// Pending decisions not yet applied.
    pub pending_decisions: usize,

    /// Age of oldest buffered snapshot.
    pub oldest_age: Option<Duration>,

    /// Current delay setting.
    pub current_delay: Duration,
}

/// Manages delayed execution flow.
pub struct DelayedExecution {
    buffer: DelayBuffer,
    state_rx: mpsc::Receiver<StateSnapshot>,
    decision_rx: mpsc::Receiver<DecisionEvent>,
    output_tx: mpsc::Sender<BufferedState>,
}

impl DelayedExecution {
    /// Create a new delayed execution manager.
    pub fn new(
        config: DelayConfig,
        state_rx: mpsc::Receiver<StateSnapshot>,
        decision_rx: mpsc::Receiver<DecisionEvent>,
        output_tx: mpsc::Sender<BufferedState>,
    ) -> Self {
        Self {
            buffer: DelayBuffer::new(config),
            state_rx,
            decision_rx,
            output_tx,
        }
    }

    /// Run the delayed execution loop.
    pub async fn run(mut self) -> MountainResult<()> {
        loop {
            tokio::select! {
                // Receive new state snapshots
                Some(snapshot) = self.state_rx.recv() => {
                    self.buffer.push(snapshot).await?;
                }

                // Receive decisions
                Some(decision) = self.decision_rx.recv() => {
                    self.buffer.add_decision(decision).await;
                }

                // Release ready states
                _ = async {
                    if let Some(state) = self.buffer.pop_ready().await {
                        let _ = self.output_tx.send(state).await;
                    }
                    sleep(Duration::from_millis(1)).await;
                } => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_delay_buffer_basic() {
        let config = DelayConfig {
            delay_ms: 100,
            ..Default::default()
        };
        let buffer = DelayBuffer::new(config);

        let snapshot = StateSnapshot::default();
        buffer.push(snapshot).await.unwrap();

        // Not ready immediately
        assert!(buffer.pop_ready().await.is_none());

        // Wait for delay
        sleep(Duration::from_millis(150)).await;

        // Now should be ready
        let state = buffer.pop_ready().await;
        assert!(state.is_some());
    }

    #[tokio::test]
    async fn test_buffer_ordering() {
        let config = DelayConfig {
            delay_ms: 50,
            ..Default::default()
        };
        let buffer = DelayBuffer::new(config);

        // Push multiple states
        for i in 0..3 {
            let snapshot = StateSnapshot::new().with_stdout(format!("{}", i));
            buffer.push(snapshot).await.unwrap();
            sleep(Duration::from_millis(10)).await;
        }

        sleep(Duration::from_millis(100)).await;

        // Should come out in order
        let first = buffer.pop_ready().await.unwrap();
        assert_eq!(first.snapshot.stdout, "0");

        let second = buffer.pop_ready().await.unwrap();
        assert_eq!(second.snapshot.stdout, "1");
    }

    #[tokio::test]
    async fn test_decision_attachment() {
        let config = DelayConfig {
            delay_ms: 50,
            ..Default::default()
        };
        let buffer = DelayBuffer::new(config);

        let snapshot = StateSnapshot::default();
        let ts = snapshot.timestamp_ms;
        buffer.push(snapshot).await.unwrap();

        // Add a decision targeting this timestamp
        let decision = DecisionEvent::new(
            crate::cascade::ControlDecision::default(),
            "test",
        ).with_target(ts);
        buffer.add_decision(decision).await;

        sleep(Duration::from_millis(100)).await;

        let state = buffer.pop_ready().await.unwrap();
        assert!(state.decision.is_some());
    }
}
