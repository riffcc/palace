//! Conductor: Recursive interview system for human-AI collaboration.
//!
//! Conductor allows all uncertainties to be resolved by directly asking the user
//! through a recursive interview process. Questions branch and regenerate as you
//! answer, with caching and pruning to keep interactions efficient.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                         CONDUCTOR                                │
//! │                                                                  │
//! │  ┌────────────┐  ┌────────────┐  ┌────────────┐  ┌───────────┐ │
//! │  │ Interview  │──│  Question  │──│   Option   │──│  Answer   │ │
//! │  │   Tree     │  │  Generator │  │  Evaluator │  │  Handler  │ │
//! │  └────────────┘  └────────────┘  └────────────┘  └───────────┘ │
//! │        │                                               │        │
//! │        └───────────────────────────────────────────────┘        │
//! │                              │                                   │
//! │  ┌───────────────────────────▼────────────────────────────────┐ │
//! │  │                    Gamepad Interface                        │ │
//! │  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────────────┐│ │
//! │  │  │ Radial  │  │ Radial  │  │Touchpad │  │   Output        ││ │
//! │  │  │  Left   │  │  Right  │  │ Surface │  │   Stream        ││ │
//! │  │  │(Topic)  │  │(Intent) │  │(Select) │  │   (L2/R2)       ││ │
//! │  │  └─────────┘  └─────────┘  └─────────┘  └─────────────────┘│ │
//! │  └────────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Gamepad Controls
//!
//! - **RB/LB**: Toggle between visible AI agents
//! - **L2/R2**: Control output granularity (context depth)
//! - **X + Left Stick**: Topic/Focus radial menu (L3 toggles mode)
//! - **X + Right Stick**: Intent/Strategy radial menu (R3 toggles mode)
//! - **Touchpad**: Dynamic surface for selection, flinging, combining
//! - **A/B**: Confirm/Back

mod error;
mod gamepad;
mod interview;
mod output;
mod question;
mod radial;
mod remote;
mod touchpad;

pub use error::{ConductorError, ConductorResult};
pub use gamepad::{GamepadInput, GamepadState, HapticFeedback, PS5Controller};
pub use interview::{Interview, InterviewState, InterviewTree};
pub use output::{AgentOutput, OutputGranularity, OutputStream};
pub use question::{Answer, Question, QuestionCache, QuestionOption};
pub use radial::{RadialMenu, RadialMode, RadialSelection};
pub use remote::{
    ControlFocus, DualScreenLayout, KnownNodes, MultiplayerSettings, NodeCapabilities, NodeRole,
    PalaceNode, RemoteAction, RemoteSession, ScreenContent, SwapRequest,
};
pub use touchpad::{TouchpadGesture, TouchpadState, TouchpadSurface};

use std::sync::Arc;
use tokio::sync::RwLock;

/// The main Conductor orchestrator.
pub struct Conductor {
    interview: Arc<RwLock<InterviewTree>>,
    gamepad: Arc<RwLock<Option<PS5Controller>>>,
    output_stream: Arc<RwLock<OutputStream>>,
    state: Arc<RwLock<ConductorState>>,
}

/// Overall Conductor state.
#[derive(Debug, Clone, Default)]
pub struct ConductorState {
    /// Whether actively interviewing.
    pub interviewing: bool,

    /// Current granularity level.
    pub granularity: OutputGranularity,

    /// Visible agent index.
    pub visible_agent: usize,

    /// Total agents available.
    pub total_agents: usize,

    /// Whether X is held (radial menu active).
    pub radial_active: bool,

    /// Left radial current mode.
    pub left_radial_mode: RadialMode,

    /// Right radial current mode.
    pub right_radial_mode: RadialMode,
}

impl Conductor {
    /// Create a new Conductor.
    pub fn new() -> Self {
        Self {
            interview: Arc::new(RwLock::new(InterviewTree::new())),
            gamepad: Arc::new(RwLock::new(None)),
            output_stream: Arc::new(RwLock::new(OutputStream::new())),
            state: Arc::new(RwLock::new(ConductorState::default())),
        }
    }

    /// Create a builder.
    pub fn builder() -> ConductorBuilder {
        ConductorBuilder::default()
    }

    /// Initialize gamepad support.
    pub async fn init_gamepad(&self) -> ConductorResult<()> {
        let controller = PS5Controller::new()?;
        let mut gamepad = self.gamepad.write().await;
        *gamepad = Some(controller);
        Ok(())
    }

    /// Start an interview.
    pub async fn start_interview(&self, topic: impl Into<String>) -> ConductorResult<()> {
        let mut interview = self.interview.write().await;
        interview.start(topic.into());

        let mut state = self.state.write().await;
        state.interviewing = true;

        Ok(())
    }

    /// Get the current question.
    pub async fn current_question(&self) -> Option<Question> {
        let interview = self.interview.read().await;
        interview.current_question()
    }

    /// Submit an answer.
    pub async fn answer(&self, answer: Answer) -> ConductorResult<()> {
        let mut interview = self.interview.write().await;
        interview.submit_answer(answer)?;
        Ok(())
    }

    /// Process gamepad input.
    pub async fn process_input(&self) -> ConductorResult<Option<ConductorAction>> {
        let gamepad = self.gamepad.read().await;
        let Some(ref controller) = *gamepad else {
            return Ok(None);
        };

        let input = controller.poll()?;
        let mut state = self.state.write().await;

        // RB/LB - toggle visible agent
        if input.rb_pressed {
            state.visible_agent = (state.visible_agent + 1) % state.total_agents.max(1);
            return Ok(Some(ConductorAction::SwitchAgent(state.visible_agent)));
        }
        if input.lb_pressed {
            state.visible_agent = state
                .visible_agent
                .checked_sub(1)
                .unwrap_or(state.total_agents.saturating_sub(1));
            return Ok(Some(ConductorAction::SwitchAgent(state.visible_agent)));
        }

        // L2/R2 - adjust granularity
        if input.l2 > 0.5 {
            state.granularity = state.granularity.decrease();
            return Ok(Some(ConductorAction::AdjustGranularity(state.granularity)));
        }
        if input.r2 > 0.5 {
            state.granularity = state.granularity.increase();
            return Ok(Some(ConductorAction::AdjustGranularity(state.granularity)));
        }

        // X button - toggle radial menus
        state.radial_active = input.x_held;

        // L3/R3 - toggle radial modes
        if input.l3_pressed {
            state.left_radial_mode = state.left_radial_mode.toggle();
        }
        if input.r3_pressed {
            state.right_radial_mode = state.right_radial_mode.toggle();
        }

        // Radial menu selection with sticks while X held
        if state.radial_active {
            if let Some(selection) = self.process_radial_input(&input, &state).await {
                return Ok(Some(ConductorAction::RadialSelect(selection)));
            }
        }

        // Touchpad gestures
        if let Some(gesture) = input.touchpad_gesture {
            return Ok(Some(ConductorAction::TouchpadGesture(gesture)));
        }

        // A/B for confirm/back
        if input.a_pressed {
            return Ok(Some(ConductorAction::Confirm));
        }
        if input.b_pressed {
            return Ok(Some(ConductorAction::Back));
        }

        Ok(None)
    }

    /// Process radial input.
    async fn process_radial_input(
        &self,
        input: &GamepadInput,
        state: &ConductorState,
    ) -> Option<RadialSelection> {
        let left_angle = input.left_stick_angle();
        let right_angle = input.right_stick_angle();

        // Need significant stick deflection
        let left_magnitude = input.left_stick_magnitude();
        let right_magnitude = input.right_stick_magnitude();

        if left_magnitude > 0.5 {
            return Some(RadialSelection {
                menu: radial::RadialMenuSide::Left,
                mode: state.left_radial_mode,
                angle: left_angle,
                magnitude: left_magnitude,
            });
        }

        if right_magnitude > 0.5 {
            return Some(RadialSelection {
                menu: radial::RadialMenuSide::Right,
                mode: state.right_radial_mode,
                angle: right_angle,
                magnitude: right_magnitude,
            });
        }

        None
    }

    /// Get current output stream state.
    pub async fn output_state(&self) -> impl std::ops::Deref<Target = OutputStream> + '_ {
        self.output_stream.read().await
    }

    /// Add an agent output.
    pub async fn add_agent_output(&self, agent_id: usize, output: AgentOutput) {
        let mut stream = self.output_stream.write().await;
        stream.add_output(agent_id, output);
    }

    /// Run the conductor loop.
    pub async fn run(&self) -> ConductorResult<()> {
        loop {
            if let Some(action) = self.process_input().await? {
                self.handle_action(action).await?;
            }

            // Small delay
            tokio::time::sleep(std::time::Duration::from_millis(16)).await; // ~60fps
        }
    }

    /// Handle a conductor action.
    async fn handle_action(&self, action: ConductorAction) -> ConductorResult<()> {
        tracing::debug!("Conductor action: {:?}", action);

        match action {
            ConductorAction::SwitchAgent(idx) => {
                let mut state = self.state.write().await;
                state.visible_agent = idx;
            }
            ConductorAction::AdjustGranularity(level) => {
                let mut state = self.state.write().await;
                state.granularity = level;
            }
            ConductorAction::Confirm => {
                // Submit current selection
            }
            ConductorAction::Back => {
                // Go back in interview
            }
            ConductorAction::RadialSelect(selection) => {
                // Handle radial selection
                tracing::info!("Radial selection: {:?}", selection);
            }
            ConductorAction::TouchpadGesture(gesture) => {
                // Handle touchpad gesture
                tracing::info!("Touchpad gesture: {:?}", gesture);
            }
        }

        Ok(())
    }
}

impl Default for Conductor {
    fn default() -> Self {
        Self::new()
    }
}

/// Action from conductor processing.
#[derive(Debug, Clone)]
pub enum ConductorAction {
    /// Switch visible agent.
    SwitchAgent(usize),
    /// Adjust output granularity.
    AdjustGranularity(OutputGranularity),
    /// Confirm current selection.
    Confirm,
    /// Go back.
    Back,
    /// Radial menu selection.
    RadialSelect(RadialSelection),
    /// Touchpad gesture.
    TouchpadGesture(TouchpadGesture),
}

/// Builder for Conductor.
#[derive(Default)]
pub struct ConductorBuilder {
    enable_gamepad: bool,
    initial_topic: Option<String>,
}

impl ConductorBuilder {
    /// Enable gamepad support.
    pub fn with_gamepad(mut self) -> Self {
        self.enable_gamepad = true;
        self
    }

    /// Set initial interview topic.
    pub fn with_topic(mut self, topic: impl Into<String>) -> Self {
        self.initial_topic = Some(topic.into());
        self
    }

    /// Build the Conductor.
    pub async fn build(self) -> ConductorResult<Conductor> {
        let conductor = Conductor::new();

        if self.enable_gamepad {
            conductor.init_gamepad().await?;
        }

        if let Some(topic) = self.initial_topic {
            conductor.start_interview(topic).await?;
        }

        Ok(conductor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_conductor_creation() {
        let conductor = Conductor::new();
        let state = conductor.state.read().await;
        assert!(!state.interviewing);
    }

    #[tokio::test]
    async fn test_start_interview() {
        let conductor = Conductor::new();
        conductor.start_interview("Test topic").await.unwrap();

        let state = conductor.state.read().await;
        assert!(state.interviewing);
    }
}
