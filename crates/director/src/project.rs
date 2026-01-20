//! Project management for Director.

use crate::goals::{Goal, GoalStatus};
use crate::issues::Issue;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Project configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Project name.
    pub name: String,

    /// Project root path.
    pub root_path: PathBuf,

    /// Project description.
    pub description: Option<String>,

    /// Repository URL.
    pub repository_url: Option<String>,

    /// Plane.so project ID.
    pub plane_project_id: Option<String>,

    /// Plane.so workspace slug.
    pub plane_workspace: Option<String>,

    /// CI level for this project.
    pub ci_level: String,

    /// Whether to auto-create PRs.
    pub auto_create_prs: bool,

    /// Whether to auto-merge when CI passes.
    pub auto_merge: bool,

    /// Human review required for these labels.
    pub require_human_review_labels: Vec<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            root_path: PathBuf::from("."),
            description: None,
            repository_url: None,
            plane_project_id: None,
            plane_workspace: None,
            ci_level: "basic".into(),
            auto_create_prs: true,
            auto_merge: false,
            require_human_review_labels: vec!["needs-review".into(), "breaking-change".into()],
        }
    }
}

/// A project managed by Director.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Project configuration.
    pub config: ProjectConfig,

    /// Project goals.
    pub goals: Vec<Goal>,

    /// Project issues.
    pub issues: Vec<Issue>,

    /// Active branches.
    pub branches: Vec<String>,
}

impl Project {
    /// Create a new project.
    pub fn new(config: ProjectConfig) -> Self {
        Self {
            config,
            goals: vec![],
            issues: vec![],
            branches: vec![],
        }
    }

    /// Add a goal.
    pub fn add_goal(&mut self, goal: Goal) {
        self.goals.push(goal);
    }

    /// Add an issue.
    pub fn add_issue(&mut self, issue: Issue) {
        self.issues.push(issue);
    }

    /// Get active goals.
    pub fn active_goals(&self) -> Vec<&Goal> {
        self.goals
            .iter()
            .filter(|g| g.status == GoalStatus::Pending || g.status == GoalStatus::InProgress)
            .collect()
    }

    /// Get completed goals.
    pub fn completed_goals(&self) -> Vec<&Goal> {
        self.goals
            .iter()
            .filter(|g| g.status == GoalStatus::Completed)
            .collect()
    }

    /// Check if all goals are complete.
    pub fn all_goals_complete(&self) -> bool {
        self.goals.iter().all(|g| g.status == GoalStatus::Completed)
    }

    /// Get goal by ID.
    pub fn get_goal(&self, id: uuid::Uuid) -> Option<&Goal> {
        self.goals.iter().find(|g| g.id == id)
    }

    /// Get mutable goal by ID.
    pub fn get_goal_mut(&mut self, id: uuid::Uuid) -> Option<&mut Goal> {
        self.goals.iter_mut().find(|g| g.id == id)
    }

    /// Get issue by ID.
    pub fn get_issue(&self, id: uuid::Uuid) -> Option<&Issue> {
        self.issues.iter().find(|i| i.id == id)
    }

    /// Get mutable issue by ID.
    pub fn get_issue_mut(&mut self, id: uuid::Uuid) -> Option<&mut Issue> {
        self.issues.iter_mut().find(|i| i.id == id)
    }

    /// Get issues for a goal.
    pub fn issues_for_goal(&self, goal_id: uuid::Uuid) -> Vec<&Issue> {
        self.issues
            .iter()
            .filter(|i| i.goal_id == Some(goal_id))
            .collect()
    }

    /// Calculate overall progress.
    pub fn progress(&self) -> f32 {
        if self.goals.is_empty() {
            return 1.0;
        }
        let total_progress: u32 = self.goals.iter().map(|g| g.progress as u32).sum();
        total_progress as f32 / (self.goals.len() as f32 * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_goals() {
        let mut project = Project::new(ProjectConfig {
            name: "test".into(),
            ..Default::default()
        });

        let goal1 = Goal::new("Goal 1");
        let mut goal2 = Goal::new("Goal 2");
        goal2.complete();

        project.add_goal(goal1);
        project.add_goal(goal2);

        assert_eq!(project.active_goals().len(), 1);
        assert_eq!(project.completed_goals().len(), 1);
        assert!(!project.all_goals_complete());
    }

    #[test]
    fn test_project_progress() {
        let mut project = Project::new(ProjectConfig {
            name: "test".into(),
            ..Default::default()
        });

        let mut goal1 = Goal::new("Goal 1");
        goal1.set_progress(50);

        let mut goal2 = Goal::new("Goal 2");
        goal2.set_progress(100);

        project.add_goal(goal1);
        project.add_goal(goal2);

        assert_eq!(project.progress(), 0.75);
    }
}
