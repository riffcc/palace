//! Planning and task breakdown for Director.

use crate::error::{DirectorError, DirectorResult};
use crate::goals::{Goal, GoalStatus};
use crate::issues::Issue;
use crate::project::Project;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A step in the execution plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// Step identifier.
    pub id: Uuid,

    /// Step description.
    pub description: String,

    /// Related issue if any.
    pub issue_id: Option<Uuid>,

    /// Related goal if any.
    pub goal_id: Option<Uuid>,

    /// Action type.
    pub action: StepAction,

    /// Whether this step is complete.
    pub complete: bool,

    /// Step result if executed.
    pub result: Option<StepResult>,
}

impl PlanStep {
    /// Create a new plan step.
    pub fn new(description: impl Into<String>, action: StepAction) -> Self {
        Self {
            id: Uuid::new_v4(),
            description: description.into(),
            issue_id: None,
            goal_id: None,
            action,
            complete: false,
            result: None,
        }
    }

    /// Link to an issue.
    pub fn for_issue(mut self, issue_id: Uuid) -> Self {
        self.issue_id = Some(issue_id);
        self
    }

    /// Link to a goal.
    pub fn for_goal(mut self, goal_id: Uuid) -> Self {
        self.goal_id = Some(goal_id);
        self
    }

    /// Mark as complete with result.
    pub fn mark_complete(&mut self, result: StepResult) {
        self.complete = true;
        self.result = Some(result);
    }
}

/// Type of action for a plan step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepAction {
    /// Create an issue in the tracker.
    CreateIssue { issue: Box<Issue> },

    /// Implement code changes.
    Implement {
        files: Vec<String>,
        description: String,
    },

    /// Run tests.
    Test { test_pattern: Option<String> },

    /// Run the build.
    Build { release: bool },

    /// Create a pull request.
    CreatePR { title: String, branch: String },

    /// Request human review.
    HumanReview { question: String },

    /// Deploy changes.
    Deploy { environment: String },

    /// Research/investigate.
    Research { topic: String },

    /// Custom action.
    Custom { action_type: String, params: serde_json::Value },
}

/// Result of executing a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// Whether the step succeeded.
    pub success: bool,

    /// Result message.
    pub message: String,

    /// Any outputs produced.
    pub outputs: std::collections::HashMap<String, serde_json::Value>,

    /// Duration in milliseconds.
    pub duration_ms: u64,
}

impl StepResult {
    /// Create a success result.
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            outputs: std::collections::HashMap::new(),
            duration_ms: 0,
        }
    }

    /// Create a failure result.
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            outputs: std::collections::HashMap::new(),
            duration_ms: 0,
        }
    }

    /// Add an output.
    pub fn with_output(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.outputs.insert(key.into(), value);
        self
    }

    /// Set duration.
    pub fn with_duration(mut self, ms: u64) -> Self {
        self.duration_ms = ms;
        self
    }
}

/// An execution plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Plan identifier.
    pub id: Uuid,

    /// Plan title.
    pub title: String,

    /// Goals this plan addresses.
    pub goal_ids: Vec<Uuid>,

    /// Ordered steps.
    pub steps: Vec<PlanStep>,

    /// Current step index.
    pub current_step: usize,

    /// Created timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Plan {
    /// Create a new plan.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            goal_ids: vec![],
            steps: vec![],
            current_step: 0,
            created_at: chrono::Utc::now(),
        }
    }

    /// Add a goal.
    pub fn for_goals(mut self, goal_ids: Vec<Uuid>) -> Self {
        self.goal_ids = goal_ids;
        self
    }

    /// Add a step.
    pub fn with_step(mut self, step: PlanStep) -> Self {
        self.steps.push(step);
        self
    }

    /// Get the next incomplete step.
    pub fn next_step(&mut self) -> Option<PlanStep> {
        while self.current_step < self.steps.len() {
            let step = &self.steps[self.current_step];
            if !step.complete {
                let step = step.clone();
                self.current_step += 1;
                return Some(step);
            }
            self.current_step += 1;
        }
        None
    }

    /// Get progress percentage.
    pub fn progress(&self) -> f32 {
        if self.steps.is_empty() {
            return 1.0;
        }
        let complete = self.steps.iter().filter(|s| s.complete).count();
        complete as f32 / self.steps.len() as f32
    }

    /// Check if plan is complete.
    pub fn is_complete(&self) -> bool {
        self.steps.iter().all(|s| s.complete)
    }
}

/// The Planner creates execution plans from goals.
pub struct Planner {
    // Would contain LLM client for intelligent planning
}

impl Planner {
    /// Create a new planner.
    pub fn new() -> Self {
        Self {}
    }

    /// Create a plan for the project's current goals.
    pub async fn create_plan(&self, project: &Project) -> DirectorResult<Plan> {
        let active_goals: Vec<_> = project
            .goals
            .iter()
            .filter(|g| g.status == GoalStatus::Pending || g.status == GoalStatus::InProgress)
            .collect();

        if active_goals.is_empty() {
            return Err(DirectorError::Planning("No active goals".into()));
        }

        // In a real implementation, this would use the LLM to:
        // 1. Analyze the goals
        // 2. Break them down into issues
        // 3. Create an ordered execution plan
        // 4. Consider dependencies and priorities

        let mut plan = Plan::new(format!("Plan for {} goals", active_goals.len()))
            .for_goals(active_goals.iter().map(|g| g.id).collect());

        // Create placeholder steps for each goal
        for goal in active_goals {
            // Research step
            plan = plan.with_step(
                PlanStep::new(
                    format!("Research: {}", goal.title),
                    StepAction::Research {
                        topic: goal.title.clone(),
                    },
                )
                .for_goal(goal.id),
            );

            // Implementation step
            plan = plan.with_step(
                PlanStep::new(
                    format!("Implement: {}", goal.title),
                    StepAction::Implement {
                        files: vec![],
                        description: goal.description.clone().unwrap_or_default(),
                    },
                )
                .for_goal(goal.id),
            );

            // Test step
            plan = plan.with_step(
                PlanStep::new(format!("Test: {}", goal.title), StepAction::Test { test_pattern: None })
                    .for_goal(goal.id),
            );

            // Build step
            plan = plan.with_step(
                PlanStep::new(format!("Build for: {}", goal.title), StepAction::Build { release: false })
                    .for_goal(goal.id),
            );
        }

        Ok(plan)
    }

    /// Replan based on current state and failures.
    pub async fn replan(&self, _project: &Project, _current_plan: &Plan, _failure: &StepResult) -> DirectorResult<Plan> {
        // In a real implementation, this would:
        // 1. Analyze what went wrong
        // 2. Determine if we need to change approach
        // 3. Create a new plan or modify the existing one
        Err(DirectorError::Planning("Replanning not yet implemented".into()))
    }
}

impl Default for Planner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_step() {
        let step = PlanStep::new("Test step", StepAction::Test { test_pattern: None });
        assert!(!step.complete);
    }

    #[test]
    fn test_plan_progress() {
        let mut plan = Plan::new("Test plan")
            .with_step(PlanStep::new("Step 1", StepAction::Build { release: false }))
            .with_step(PlanStep::new("Step 2", StepAction::Build { release: false }));

        assert_eq!(plan.progress(), 0.0);

        plan.steps[0].mark_complete(StepResult::success("Done"));
        assert_eq!(plan.progress(), 0.5);

        plan.steps[1].mark_complete(StepResult::success("Done"));
        assert_eq!(plan.progress(), 1.0);
        assert!(plan.is_complete());
    }

    #[test]
    fn test_plan_next_step() {
        let mut plan = Plan::new("Test")
            .with_step(PlanStep::new("Step 1", StepAction::Build { release: false }))
            .with_step(PlanStep::new("Step 2", StepAction::Test { test_pattern: None }));

        let step1 = plan.next_step().unwrap();
        assert_eq!(step1.description, "Step 1");

        let step2 = plan.next_step().unwrap();
        assert_eq!(step2.description, "Step 2");

        assert!(plan.next_step().is_none());
    }
}
