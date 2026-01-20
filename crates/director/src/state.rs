//! State tracking for Director.

use crate::planner::Plan;
use serde::{Deserialize, Serialize};

/// State of the Director.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DirectorState {
    /// Current execution plan.
    pub current_plan: Option<Plan>,

    /// Number of steps completed.
    pub steps_completed: u64,

    /// Number of steps failed.
    pub steps_failed: u64,

    /// Whether currently executing.
    pub executing: bool,

    /// Last error message.
    pub last_error: Option<String>,

    /// Project metrics.
    pub metrics: ProjectMetrics,
}

impl DirectorState {
    /// Create a new state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get success rate.
    pub fn success_rate(&self) -> f32 {
        let total = self.steps_completed + self.steps_failed;
        if total == 0 {
            return 1.0;
        }
        self.steps_completed as f32 / total as f32
    }

    /// Record a success.
    pub fn record_success(&mut self) {
        self.steps_completed += 1;
    }

    /// Record a failure.
    pub fn record_failure(&mut self, error: impl Into<String>) {
        self.steps_failed += 1;
        self.last_error = Some(error.into());
    }
}

/// Metrics about the project.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectMetrics {
    /// Total goals.
    pub total_goals: u32,

    /// Completed goals.
    pub completed_goals: u32,

    /// Total issues.
    pub total_issues: u32,

    /// Completed issues.
    pub completed_issues: u32,

    /// Lines of code changed.
    pub lines_changed: u64,

    /// Files modified.
    pub files_modified: u32,

    /// Tests added.
    pub tests_added: u32,

    /// Test coverage percentage.
    pub test_coverage: f32,

    /// Build success rate.
    pub build_success_rate: f32,

    /// Average time per task (ms).
    pub avg_task_time_ms: u64,

    /// Total LLM tokens used.
    pub llm_tokens_used: u64,

    /// Human interventions required.
    pub human_interventions: u32,
}

impl ProjectMetrics {
    /// Create new metrics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Goal completion rate.
    pub fn goal_completion_rate(&self) -> f32 {
        if self.total_goals == 0 {
            return 1.0;
        }
        self.completed_goals as f32 / self.total_goals as f32
    }

    /// Issue completion rate.
    pub fn issue_completion_rate(&self) -> f32 {
        if self.total_issues == 0 {
            return 1.0;
        }
        self.completed_issues as f32 / self.total_issues as f32
    }

    /// Update from project state.
    pub fn update_from_project(&mut self, project: &crate::project::Project) {
        self.total_goals = project.goals.len() as u32;
        self.completed_goals = project.completed_goals().len() as u32;
        self.total_issues = project.issues.len() as u32;
        self.completed_issues = project
            .issues
            .iter()
            .filter(|i| i.status == crate::issues::IssueStatus::Done)
            .count() as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_director_state_success_rate() {
        let mut state = DirectorState::new();

        // No steps yet - 100% success
        assert_eq!(state.success_rate(), 1.0);

        state.record_success();
        state.record_success();
        state.record_failure("test error");

        assert!((state.success_rate() - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_project_metrics() {
        let mut metrics = ProjectMetrics::new();
        metrics.total_goals = 10;
        metrics.completed_goals = 5;

        assert_eq!(metrics.goal_completion_rate(), 0.5);
    }
}
