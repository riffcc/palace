//! Mountain: Time-delayed cascading LLM control for realtime program automation.
//!
//! Mountain enables realtime AI control of programs by compensating for LLM
//! response latency through time-delayed execution and hierarchical model
//! orchestration.
//!
//! # Architecture
//!
//! ```text
//! REALTIME STATE ──────────────────────────────────────────────►
//!      │
//!      ├──► Tiny Local (100ms) ────┐
//!      ├──► Medium Local (500ms) ──┼──► CASCADE MERGE
//!      ├──► Fast Cloud (800ms) ────┤         │
//!      └──► Big Cloud (frozen) ────┴─────────┘
//!                                            │
//!                                            ▼
//!                    WASM EXECUTION (delayed 2-3 seconds)
//! ```
//!
//! # Key Concepts
//!
//! - **Time Delay Buffer**: Execution runs X seconds behind realtime
//! - **Model Cascade**: Fast models inform slower, smarter models
//! - **Speculative Prefetch**: Small models pre-analyze for big models
//! - **Streaming Response**: Big model responds while still processing
//!
//! # Example
//!
//! ```rust,ignore
//! use mountain::{Mountain, ModelTier, ProgramController};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mountain = Mountain::builder()
//!         .add_tier(ModelTier::local("nvidia_orchestrator-8b", 100))
//!         .add_tier(ModelTier::local("devstral-small-24b", 500))
//!         .add_tier(ModelTier::cloud("glm-4.7-flash", 800))
//!         .add_tier(ModelTier::cloud("glm-4.7", 2000).frozen())
//!         .delay_buffer_ms(2500)
//!         .build()?;
//!
//!     let controller = ProgramController::new(mountain);
//!     controller.attach_to_process(process).await?;
//!
//!     Ok(())
//! }
//! ```

mod cascade;
mod confidence;
mod controller;
mod delay;
mod error;
mod model;
mod state;
mod stream;

pub mod benchmarks;

pub use cascade::{Cascade, CascadeBuilder, CascadeResult, ControlDecision, ModelResponse};
pub use confidence::{
    AssuranceLevel, ConfidenceSliderState, HapticPattern, OverclockModel, SliderAction,
};
pub use controller::{
    Controllable, ControllerBuilder, ControllerConfig, ControllerState, ControllerStats,
    ProgramController,
};
pub use delay::{BufferedState, BufferStats, DelayBuffer, DelayConfig, DelayedExecution};
pub use error::{MountainError, MountainResult};
pub use model::{ModelConfig, ModelTier, ModelType};
pub use state::{ProgramState, StateEvent, StateSnapshot, StateStream};
pub use stream::{DecisionEvent, DecisionReceiver, LLMOutput, LLMOutputStream, LLMOutputType, StateEmitter};

/// Main Mountain orchestrator.
pub struct Mountain {
    cascade: Cascade,
    delay_buffer_ms: u64,
}

impl Mountain {
    /// Create a new Mountain builder.
    pub fn builder() -> MountainBuilder {
        MountainBuilder::new()
    }

    /// Get the cascade.
    pub fn cascade(&self) -> &Cascade {
        &self.cascade
    }

    /// Get the delay buffer in milliseconds.
    pub fn delay_buffer_ms(&self) -> u64 {
        self.delay_buffer_ms
    }
}

/// Builder for Mountain configuration.
pub struct MountainBuilder {
    tiers: Vec<ModelTier>,
    delay_buffer_ms: u64,
}

impl MountainBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            tiers: vec![],
            delay_buffer_ms: 2000, // 2 second default
        }
    }

    /// Add a model tier.
    pub fn add_tier(mut self, tier: ModelTier) -> Self {
        self.tiers.push(tier);
        self
    }

    /// Set the delay buffer in milliseconds.
    pub fn delay_buffer_ms(mut self, ms: u64) -> Self {
        self.delay_buffer_ms = ms;
        self
    }

    /// Build the Mountain.
    pub fn build(self) -> MountainResult<Mountain> {
        if self.tiers.is_empty() {
            return Err(MountainError::Config("No model tiers configured".into()));
        }

        // Sort tiers by expected latency
        let mut tiers = self.tiers;
        tiers.sort_by_key(|t| t.expected_latency_ms);

        Ok(Mountain {
            cascade: Cascade::new(tiers),
            delay_buffer_ms: self.delay_buffer_ms,
        })
    }
}

impl Default for MountainBuilder {
    fn default() -> Self {
        Self::new()
    }
}
