//! Director: Autonomous project management for Palace.
//!
//! The Director acts as an AI project manager, orchestrating development
//! according to goals. It creates issues, prioritizes work, coordinates
//! the development lifecycle, and manages human/AI collaboration.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                      DIRECTOR                            │
//! │                                                          │
//! │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐ │
//! │  │  Goals   │──│ Planner  │──│ Executor │──│ Monitor │ │
//! │  └──────────┘  └──────────┘  └──────────┘  └─────────┘ │
//! │       │             │             │             │       │
//! │       └─────────────┴─────────────┴─────────────┘       │
//! │                         │                                │
//! │  ┌──────────────────────▼───────────────────────────┐   │
//! │  │              Project State                        │   │
//! │  │  Issues │ PRs │ Tasks │ Decisions │ Metrics      │   │
//! │  └──────────────────────────────────────────────────┘   │
//! │                         │                                │
//! └─────────────────────────┼────────────────────────────────┘
//!                           │
//!           ┌───────────────┼───────────────┐
//!           ▼               ▼               ▼
//!      ┌─────────┐    ┌─────────┐    ┌─────────┐
//!      │ Plane   │    │ GitHub  │    │  Human  │
//!      │   .so   │    │   API   │    │  Review │
//!      └─────────┘    └─────────┘    └─────────┘
//! ```
//!
//! # Key Concepts
//!
//! - **Goals**: High-level objectives the project should achieve
//! - **Planner**: Breaks goals into actionable tasks and issues
//! - **Executor**: Coordinates AI agents to complete tasks
//! - **Monitor**: Tracks progress and adjusts plans dynamically
//! - **Human Loop**: Escalates decisions that need human input

mod agent_tools;
mod control;
mod daemon;
mod director_tool;
mod pool;
mod error;
mod executor;
mod goals;
mod issues;
mod model_ladder;
mod multi_model;
mod orchestrator;
mod planner;
mod project;
mod session;
mod session_executor;
mod skill_finder;
mod state;
mod zulip_reactor;
mod zulip_reporter;
mod zulip_stream;
mod recursive_survey;
mod zulip_tool;

pub use agent_tools::{ZulipAgentTool, PlaneAgentTool};
pub use control::{ControlServer, ControlClient, ControlCommand, ControlResponse, GrepMatch, ToolCallResult, socket_path};
pub use director_tool::{DirectorControlTool, DirectorControlToolDef};
pub use pool::{Pool, DirectorInstance, DirectorStatus, TelepathyMessage, TelepathyKind, PoolStatus, DirectorInfo};
pub use daemon::{Daemon, DaemonConfig, DaemonEvent, ProjectGraph, GraphNode, GraphLink, NodeType};
pub use orchestrator::{DirectorConfig, DirectorPool, ModelRegistry, Route, parse_model, ContextCache, InterviewResult, PlaneIssue};
pub use error::{DirectorError, DirectorResult};
pub use executor::{Executor, ExecutorConfig};
pub use goals::{Goal, GoalPriority, GoalStatus};
pub use issues::{Issue, IssueType, IssuePriority};
pub use planner::{Plan, PlanStep, Planner, StepAction, StepResult};
pub use project::{Project, ProjectConfig};
pub use session::{
    Session, SessionEvent, SessionManager, SessionStatus, SessionStrategy, SessionTarget, SingleTarget,
    SessionLogEntry, LogLevel, ProjectSkill, SkillDirective,
    Handoff, HandoffKind,
};
pub use session_executor::{SessionExecutor, SessionExecutorConfig};
pub use skill_finder::{SkillFinder, SkillRegistry, SkillEntry, SkillContext};
pub use model_ladder::{ModelLadder, ModelTier, ModelEndpoint, ComplexitySignals, UpgradeRequest};
pub use multi_model::{MultiModelEdit, Placeholder, PlaceholderScan};
pub use state::{DirectorState, ProjectMetrics};
pub use zulip_reactor::{ZulipReactor, ZulipEvent, MessageEvent, ReactionEvent, ChannelStatus, TodoItem, emoji};
pub use zulip_reporter::{ZulipReporter, SurveyOption, TodoTask, MessageSlot};
pub use zulip_stream::{ZulipStreamer, StreamConfig, Verbosity, EventType, ChannelConfig};
pub use zulip_tool::{ZulipTool, ZulipMessage};
pub use recursive_survey::{RecursiveSurvey, SurveyDefinition, SurveyQuestion, SurveyBuilder};

use std::sync::Arc;
use tokio::sync::RwLock;

/// The main Director orchestrator.
pub struct Director {
    project: Arc<RwLock<Project>>,
    planner: Planner,
    executor: Executor,
    state: Arc<RwLock<DirectorState>>,
}

impl Director {
    /// Create a new Director for a project.
    pub fn new(project: Project) -> Self {
        let executor_config = ExecutorConfig {
            project_path: project.config.root_path.clone(),
            ..Default::default()
        };

        Self {
            project: Arc::new(RwLock::new(project)),
            planner: Planner::new(),
            executor: Executor::new(executor_config),
            state: Arc::new(RwLock::new(DirectorState::default())),
        }
    }

    /// Create a Director with custom executor config.
    pub fn with_executor(project: Project, executor_config: ExecutorConfig) -> Self {
        Self {
            project: Arc::new(RwLock::new(project)),
            planner: Planner::new(),
            executor: Executor::new(executor_config),
            state: Arc::new(RwLock::new(DirectorState::default())),
        }
    }

    /// Create a Director builder.
    pub fn builder() -> DirectorBuilder {
        DirectorBuilder::default()
    }

    /// Get the project.
    pub async fn project(&self) -> impl std::ops::Deref<Target = Project> + '_ {
        self.project.read().await
    }

    /// Get the current state.
    pub async fn state(&self) -> impl std::ops::Deref<Target = DirectorState> + '_ {
        self.state.read().await
    }

    /// Add a goal to the project.
    pub async fn add_goal(&self, goal: Goal) -> DirectorResult<()> {
        let mut project = self.project.write().await;
        project.add_goal(goal);
        Ok(())
    }

    /// Create a plan for achieving goals.
    pub async fn create_plan(&self) -> DirectorResult<Plan> {
        let project = self.project.read().await;
        let plan = self.planner.create_plan(&project).await?;
        Ok(plan)
    }

    /// Execute the next step in the plan.
    pub async fn execute_next(&self) -> DirectorResult<Option<PlanStep>> {
        let mut state = self.state.write().await;

        if let Some(mut step) = state.current_plan.as_mut().and_then(|p| p.next_step()) {
            tracing::info!("Executing step: {}", step.description);

            // Execute the step using the Executor
            let result = self.executor.execute(&step).await?;

            // Update step with result
            step.mark_complete(result.clone());

            // Update state metrics
            if result.success {
                state.steps_completed += 1;
            } else {
                state.steps_failed += 1;
            }

            // Update metrics from executor
            let exec_metrics = self.executor.metrics().await;
            state.metrics.llm_tokens_used = exec_metrics.llm_tokens_used;

            return Ok(Some(step));
        }

        Ok(None)
    }

    /// Run the director loop.
    pub async fn run(&self) -> DirectorResult<()> {
        loop {
            // Check if we have a plan
            {
                let state = self.state.read().await;
                if state.current_plan.is_none() {
                    drop(state);
                    // Create a new plan
                    let plan = self.create_plan().await?;
                    let mut state = self.state.write().await;
                    state.current_plan = Some(plan);
                }
            }

            // Execute next step
            let step = self.execute_next().await?;
            if step.is_none() {
                // Plan complete, check for more goals
                let mut state = self.state.write().await;
                state.current_plan = None;

                // Check if all goals are complete
                let project = self.project.read().await;
                if project.all_goals_complete() {
                    tracing::info!("All goals complete!");
                    break;
                }
            }

            // Small delay between steps
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        Ok(())
    }
}

/// Builder for Director configuration.
#[derive(Default)]
pub struct DirectorBuilder {
    project_config: Option<ProjectConfig>,
    goals: Vec<Goal>,
}

impl DirectorBuilder {
    /// Set the project configuration.
    pub fn project(mut self, config: ProjectConfig) -> Self {
        self.project_config = Some(config);
        self
    }

    /// Add an initial goal.
    pub fn goal(mut self, goal: Goal) -> Self {
        self.goals.push(goal);
        self
    }

    /// Build the Director.
    pub fn build(self) -> DirectorResult<Director> {
        let config = self
            .project_config
            .ok_or_else(|| DirectorError::Config("Project config required".into()))?;

        let mut project = Project::new(config);
        for goal in self.goals {
            project.add_goal(goal);
        }

        Ok(Director::new(project))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_director_builder() {
        let director = Director::builder()
            .project(ProjectConfig {
                name: "test".into(),
                ..Default::default()
            })
            .goal(Goal::new("Test goal"))
            .build()
            .unwrap();

        let project = director.project().await;
        assert_eq!(project.config.name, "test");
        assert_eq!(project.goals.len(), 1);
    }
}
