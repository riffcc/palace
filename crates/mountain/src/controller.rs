//! Program controller for Mountain.
//!
//! The controller attaches to a program and manages the bidirectional
//! flow of state streaming and decision application.

use crate::cascade::{Cascade, CascadeResult, ControlDecision};
use crate::delay::{BufferedState, DelayBuffer, DelayConfig};
use crate::error::{MountainError, MountainResult};
use crate::state::{ProgramState, StateSnapshot};
use crate::stream::{DecisionEvent, DecisionReceiver, StateEmitter};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch, Mutex, RwLock};

/// Controller state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerState {
    /// Not started.
    Idle,
    /// Running and processing.
    Running,
    /// Paused (buffer continues but no decisions applied).
    Paused,
    /// Stopped.
    Stopped,
}

/// Configuration for the program controller.
#[derive(Debug, Clone)]
pub struct ControllerConfig {
    /// Delay buffer configuration.
    pub delay: DelayConfig,

    /// Maximum cascade runs per second.
    pub max_cascade_rate: f32,

    /// Whether to auto-adjust delay based on cascade latency.
    pub auto_adjust_delay: bool,

    /// Timeout for cascade runs.
    pub cascade_timeout_ms: u64,

    /// Whether to continue on cascade errors.
    pub continue_on_error: bool,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            delay: DelayConfig::default(),
            max_cascade_rate: 30.0, // 30 decisions per second max
            auto_adjust_delay: true,
            cascade_timeout_ms: 5000,
            continue_on_error: true,
        }
    }
}

/// Statistics about controller performance.
#[derive(Debug, Clone, Default)]
pub struct ControllerStats {
    /// Total states processed.
    pub states_processed: u64,

    /// Total decisions made.
    pub decisions_made: u64,

    /// Total cascade runs.
    pub cascade_runs: u64,

    /// Average cascade latency in ms.
    pub avg_cascade_latency_ms: f32,

    /// Number of vetoed decisions.
    pub vetoes: u64,

    /// Number of cascade errors.
    pub errors: u64,

    /// Current buffer depth.
    pub buffer_depth: usize,
}

/// Trait for programs that can be controlled by Mountain.
///
/// Note: This trait does NOT require Send + Sync because many programs
/// (like emulators) use Rc internally. The controller handles thread safety
/// at a higher level.
#[async_trait::async_trait(?Send)]
pub trait Controllable {
    /// Capture current state snapshot.
    async fn capture_state(&self) -> MountainResult<StateSnapshot>;

    /// Apply a control decision.
    async fn apply_decision(&mut self, decision: &ControlDecision) -> MountainResult<()>;

    /// Pause execution (for save state systems).
    async fn pause(&mut self) -> MountainResult<()>;

    /// Resume execution.
    async fn resume(&mut self) -> MountainResult<()>;

    /// Save state to a slot.
    async fn save_state(&self, slot: u32) -> MountainResult<Vec<u8>>;

    /// Load state from a slot.
    async fn load_state(&mut self, slot: u32, data: &[u8]) -> MountainResult<()>;

    /// Check if program is still running.
    fn is_running(&self) -> bool;

    /// Get program info.
    fn program_info(&self) -> ProgramState;
}

/// The main program controller.
pub struct ProgramController<P: Controllable> {
    program: Arc<Mutex<P>>,
    cascade: Arc<Cascade>,
    config: ControllerConfig,
    delay_buffer: Arc<DelayBuffer>,
    state: Arc<RwLock<ControllerState>>,
    stats: Arc<RwLock<ControllerStats>>,
    state_emitter: Arc<StateEmitter>,
    decision_receiver: Arc<Mutex<DecisionReceiver>>,
    shutdown_tx: Option<watch::Sender<bool>>,
}

impl<P: Controllable + 'static> ProgramController<P>
where
    P: 'static,
{
    /// Create a new program controller.
    pub fn new(program: P, cascade: Cascade, config: ControllerConfig) -> Self {
        let delay_buffer = Arc::new(DelayBuffer::new(config.delay.clone()));

        Self {
            program: Arc::new(Mutex::new(program)),
            cascade: Arc::new(cascade),
            config,
            delay_buffer,
            state: Arc::new(RwLock::new(ControllerState::Idle)),
            stats: Arc::new(RwLock::new(ControllerStats::default())),
            state_emitter: Arc::new(StateEmitter::default()),
            decision_receiver: Arc::new(Mutex::new(DecisionReceiver::default())),
            shutdown_tx: None,
        }
    }

    /// Get current controller state.
    pub async fn state(&self) -> ControllerState {
        *self.state.read().await
    }

    /// Get current stats.
    pub async fn stats(&self) -> ControllerStats {
        self.stats.read().await.clone()
    }

    /// Subscribe to state updates.
    pub fn subscribe_states(&self) -> tokio::sync::broadcast::Receiver<StateSnapshot> {
        self.state_emitter.subscribe()
    }

    /// Get a sender for injecting decisions (useful for testing/debugging).
    pub async fn decision_sender(&self) -> mpsc::Sender<DecisionEvent> {
        self.decision_receiver.lock().await.sender()
    }

    /// Start the control loop.
    pub async fn start(&mut self) -> MountainResult<()> {
        let current = *self.state.read().await;
        if current == ControllerState::Running {
            return Ok(());
        }

        *self.state.write().await = ControllerState::Running;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        self.shutdown_tx = Some(shutdown_tx);

        // Spawn the main control loop using spawn_local (doesn't require Send)
        let program = self.program.clone();
        let cascade = self.cascade.clone();
        let config = self.config.clone();
        let delay_buffer = self.delay_buffer.clone();
        let state = self.state.clone();
        let stats = self.stats.clone();
        let state_emitter = self.state_emitter.clone();

        tokio::task::spawn_local(async move {
            Self::control_loop(
                program,
                cascade,
                config,
                delay_buffer,
                state,
                stats,
                state_emitter,
                shutdown_rx,
            )
            .await
        });

        Ok(())
    }

    /// The main control loop.
    async fn control_loop(
        program: Arc<Mutex<P>>,
        cascade: Arc<Cascade>,
        config: ControllerConfig,
        delay_buffer: Arc<DelayBuffer>,
        state: Arc<RwLock<ControllerState>>,
        stats: Arc<RwLock<ControllerStats>>,
        state_emitter: Arc<StateEmitter>,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        let min_interval = Duration::from_secs_f32(1.0 / config.max_cascade_rate);

        loop {
            // Check for shutdown
            if *shutdown_rx.borrow() {
                break;
            }

            // Check state
            let current_state = *state.read().await;
            match current_state {
                ControllerState::Stopped => break,
                ControllerState::Paused => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }
                ControllerState::Idle => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }
                ControllerState::Running => {}
            }

            // Capture state from program (realtime)
            let snapshot = {
                let prog = program.lock().await;
                match prog.capture_state().await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Failed to capture state: {}", e);
                        let mut st = stats.write().await;
                        st.errors += 1;
                        if !config.continue_on_error {
                            *state.write().await = ControllerState::Stopped;
                            break;
                        }
                        continue;
                    }
                }
            };

            // Emit to subscribers (realtime stream)
            state_emitter.emit(snapshot.clone()).await;

            // Push to delay buffer
            if let Err(e) = delay_buffer.push(snapshot.clone()).await {
                tracing::error!("Buffer error: {}", e);
            }

            // Run cascade on realtime state
            let cascade_start = std::time::Instant::now();
            let cascade_result = tokio::time::timeout(
                Duration::from_millis(config.cascade_timeout_ms),
                cascade.run(&snapshot),
            )
            .await;

            let cascade_latency = cascade_start.elapsed();

            match cascade_result {
                Ok(Ok(result)) => {
                    // Update stats
                    {
                        let mut st = stats.write().await;
                        st.cascade_runs += 1;
                        st.avg_cascade_latency_ms = (st.avg_cascade_latency_ms
                            * (st.cascade_runs - 1) as f32
                            + cascade_latency.as_millis() as f32)
                            / st.cascade_runs as f32;

                        if result.vetoed {
                            st.vetoes += 1;
                        }
                    }

                    // Create decision event
                    let decision_event = DecisionEvent::new(
                        result.final_decision.clone(),
                        result
                            .responses
                            .last()
                            .map(|r| r.tier_name.as_str())
                            .unwrap_or("unknown"),
                    )
                    .with_confidence(
                        result
                            .responses
                            .last()
                            .map(|r| r.confidence)
                            .unwrap_or(0.5),
                    );

                    // Add to delay buffer for application at right time
                    delay_buffer.add_decision(decision_event).await;

                    // Auto-adjust delay if enabled
                    if config.auto_adjust_delay {
                        let _ = delay_buffer.adjust_delay(cascade_latency).await;
                    }
                }
                Ok(Err(e)) => {
                    tracing::error!("Cascade error: {}", e);
                    let mut st = stats.write().await;
                    st.errors += 1;
                }
                Err(_) => {
                    tracing::warn!("Cascade timed out");
                    let mut st = stats.write().await;
                    st.errors += 1;
                }
            }

            // Check for ready buffered states and apply decisions
            while let Some(buffered) = delay_buffer.pop_ready().await {
                let mut st = stats.write().await;
                st.states_processed += 1;

                if let Some(decision) = buffered.decision {
                    st.decisions_made += 1;
                    drop(st);

                    // Apply the decision
                    let mut prog = program.lock().await;
                    if let Err(e) = prog.apply_decision(&decision.decision).await {
                        tracing::error!("Failed to apply decision: {}", e);
                        let mut st = stats.write().await;
                        st.errors += 1;
                    }
                }
            }

            // Update buffer stats
            {
                let mut st = stats.write().await;
                st.buffer_depth = delay_buffer.len().await;
            }

            // Rate limiting
            tokio::time::sleep(min_interval).await;
        }

        *state.write().await = ControllerState::Stopped;
    }

    /// Pause the controller.
    pub async fn pause(&mut self) -> MountainResult<()> {
        let mut state = self.state.write().await;
        if *state == ControllerState::Running {
            *state = ControllerState::Paused;

            // Also pause the program
            let mut prog = self.program.lock().await;
            prog.pause().await?;
        }
        Ok(())
    }

    /// Resume the controller.
    pub async fn resume(&mut self) -> MountainResult<()> {
        let mut state = self.state.write().await;
        if *state == ControllerState::Paused {
            // Resume the program first
            let mut prog = self.program.lock().await;
            prog.resume().await?;

            *state = ControllerState::Running;
        }
        Ok(())
    }

    /// Stop the controller.
    pub async fn stop(&mut self) -> MountainResult<()> {
        *self.state.write().await = ControllerState::Stopped;
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        Ok(())
    }

    /// Get program reference.
    pub fn program(&self) -> Arc<Mutex<P>> {
        self.program.clone()
    }

    /// Get the delay buffer.
    pub fn delay_buffer(&self) -> Arc<DelayBuffer> {
        self.delay_buffer.clone()
    }
}

/// Builder for program controllers.
pub struct ControllerBuilder<P: Controllable> {
    program: P,
    cascade: Option<Cascade>,
    config: ControllerConfig,
}

impl<P: Controllable + 'static> ControllerBuilder<P> {
    /// Create a new builder.
    pub fn new(program: P) -> Self {
        Self {
            program,
            cascade: None,
            config: ControllerConfig::default(),
        }
    }

    /// Set the cascade.
    pub fn with_cascade(mut self, cascade: Cascade) -> Self {
        self.cascade = Some(cascade);
        self
    }

    /// Set delay configuration.
    pub fn with_delay(mut self, delay_ms: u64) -> Self {
        self.config.delay.delay_ms = delay_ms;
        self
    }

    /// Set max cascade rate.
    pub fn with_max_rate(mut self, rate: f32) -> Self {
        self.config.max_cascade_rate = rate;
        self
    }

    /// Set cascade timeout.
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.config.cascade_timeout_ms = timeout_ms;
        self
    }

    /// Enable/disable auto delay adjustment.
    pub fn with_auto_adjust(mut self, enabled: bool) -> Self {
        self.config.auto_adjust_delay = enabled;
        self
    }

    /// Build the controller.
    pub fn build(self) -> MountainResult<ProgramController<P>> {
        let cascade = self
            .cascade
            .ok_or_else(|| MountainError::Config("Cascade is required".into()))?;

        Ok(ProgramController::new(self.program, cascade, self.config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cascade::CascadeBuilder;
    use crate::model::ModelTier;

    /// Mock controllable program for testing.
    struct MockProgram {
        state_counter: u32,
        decisions: Vec<ControlDecision>,
        running: bool,
    }

    impl MockProgram {
        fn new() -> Self {
            Self {
                state_counter: 0,
                decisions: vec![],
                running: true,
            }
        }
    }

    #[async_trait::async_trait(?Send)]
    impl Controllable for MockProgram {
        async fn capture_state(&self) -> MountainResult<StateSnapshot> {
            Ok(StateSnapshot::new().with_variable(
                "counter",
                serde_json::json!(self.state_counter),
            ))
        }

        async fn apply_decision(&mut self, decision: &ControlDecision) -> MountainResult<()> {
            self.decisions.push(decision.clone());
            Ok(())
        }

        async fn pause(&mut self) -> MountainResult<()> {
            self.running = false;
            Ok(())
        }

        async fn resume(&mut self) -> MountainResult<()> {
            self.running = true;
            Ok(())
        }

        async fn save_state(&self, _slot: u32) -> MountainResult<Vec<u8>> {
            Ok(vec![])
        }

        async fn load_state(&mut self, _slot: u32, _data: &[u8]) -> MountainResult<()> {
            Ok(())
        }

        fn is_running(&self) -> bool {
            self.running
        }

        fn program_info(&self) -> ProgramState {
            ProgramState::new("mock")
        }
    }

    #[tokio::test]
    async fn test_controller_builder() {
        let program = MockProgram::new();
        let cascade = CascadeBuilder::new()
            .add_tier(ModelTier::local("test", 100))
            .build();

        let controller = ControllerBuilder::new(program)
            .with_cascade(cascade)
            .with_delay(500)
            .build()
            .unwrap();

        assert_eq!(controller.state().await, ControllerState::Idle);
    }

    #[tokio::test]
    async fn test_controller_states() {
        let local = tokio::task::LocalSet::new();
        local.run_until(async {
            let program = MockProgram::new();
            let cascade = CascadeBuilder::new()
                .add_tier(ModelTier::local("test", 100))
                .build();

            let mut controller = ControllerBuilder::new(program)
                .with_cascade(cascade)
                .build()
                .unwrap();

            assert_eq!(controller.state().await, ControllerState::Idle);

            controller.start().await.unwrap();
            assert_eq!(controller.state().await, ControllerState::Running);

            controller.pause().await.unwrap();
            assert_eq!(controller.state().await, ControllerState::Paused);

            controller.resume().await.unwrap();
            assert_eq!(controller.state().await, ControllerState::Running);

            controller.stop().await.unwrap();
            // Give it a moment to stop
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert_eq!(controller.state().await, ControllerState::Stopped);
        }).await;
    }
}
