//! Goal management for Director.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Priority level for a goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalPriority {
    /// Low priority - nice to have.
    Low,
    /// Normal priority.
    Normal,
    /// High priority.
    High,
    /// Critical - must be done.
    Critical,
}

impl Default for GoalPriority {
    fn default() -> Self {
        GoalPriority::Normal
    }
}

/// Status of a goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    /// Not started.
    Pending,
    /// Currently being worked on.
    InProgress,
    /// Blocked on something.
    Blocked,
    /// Completed successfully.
    Completed,
    /// Cancelled or abandoned.
    Cancelled,
}

impl Default for GoalStatus {
    fn default() -> Self {
        GoalStatus::Pending
    }
}

/// A high-level goal for the project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    /// Unique identifier.
    pub id: Uuid,

    /// Goal title/summary.
    pub title: String,

    /// Detailed description.
    pub description: Option<String>,

    /// Priority level.
    pub priority: GoalPriority,

    /// Current status.
    pub status: GoalStatus,

    /// Acceptance criteria.
    pub criteria: Vec<String>,

    /// Sub-goals this depends on.
    pub dependencies: Vec<Uuid>,

    /// Parent goal if this is a sub-goal.
    pub parent: Option<Uuid>,

    /// Tags/labels.
    pub tags: Vec<String>,

    /// Created timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Last updated timestamp.
    pub updated_at: chrono::DateTime<chrono::Utc>,

    /// Completion percentage (0-100).
    pub progress: u8,
}

impl Goal {
    /// Create a new goal.
    pub fn new(title: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            description: None,
            priority: GoalPriority::default(),
            status: GoalStatus::default(),
            criteria: vec![],
            dependencies: vec![],
            parent: None,
            tags: vec![],
            created_at: now,
            updated_at: now,
            progress: 0,
        }
    }

    /// Set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: GoalPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Add acceptance criteria.
    pub fn with_criteria(mut self, criteria: impl Into<String>) -> Self {
        self.criteria.push(criteria.into());
        self
    }

    /// Add a dependency.
    pub fn depends_on(mut self, goal_id: Uuid) -> Self {
        self.dependencies.push(goal_id);
        self
    }

    /// Set parent goal.
    pub fn with_parent(mut self, parent_id: Uuid) -> Self {
        self.parent = Some(parent_id);
        self
    }

    /// Add a tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Check if this goal is complete.
    pub fn is_complete(&self) -> bool {
        self.status == GoalStatus::Completed
    }

    /// Check if this goal can be started (dependencies met).
    pub fn can_start(&self, completed_goals: &[Uuid]) -> bool {
        self.dependencies.iter().all(|d| completed_goals.contains(d))
    }

    /// Update progress.
    pub fn set_progress(&mut self, progress: u8) {
        self.progress = progress.min(100);
        self.updated_at = chrono::Utc::now();

        if self.progress == 100 && self.status != GoalStatus::Completed {
            self.status = GoalStatus::Completed;
        }
    }

    /// Mark as in progress.
    pub fn start(&mut self) {
        if self.status == GoalStatus::Pending {
            self.status = GoalStatus::InProgress;
            self.updated_at = chrono::Utc::now();
        }
    }

    /// Mark as complete.
    pub fn complete(&mut self) {
        self.status = GoalStatus::Completed;
        self.progress = 100;
        self.updated_at = chrono::Utc::now();
    }

    /// Mark as blocked.
    pub fn block(&mut self, _reason: impl Into<String>) {
        self.status = GoalStatus::Blocked;
        self.updated_at = chrono::Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_goal_creation() {
        let goal = Goal::new("Test feature")
            .with_description("A test feature")
            .with_priority(GoalPriority::High)
            .with_criteria("Must pass all tests")
            .with_tag("testing");

        assert_eq!(goal.title, "Test feature");
        assert_eq!(goal.priority, GoalPriority::High);
        assert_eq!(goal.criteria.len(), 1);
        assert_eq!(goal.tags.len(), 1);
    }

    #[test]
    fn test_goal_progress() {
        let mut goal = Goal::new("Test");
        assert_eq!(goal.progress, 0);
        assert!(!goal.is_complete());

        goal.set_progress(50);
        assert_eq!(goal.progress, 50);

        goal.set_progress(100);
        assert!(goal.is_complete());
    }

    #[test]
    fn test_goal_dependencies() {
        let goal1 = Goal::new("First");
        let goal2 = Goal::new("Second").depends_on(goal1.id);

        assert!(!goal2.can_start(&[]));
        assert!(goal2.can_start(&[goal1.id]));
    }
}
