//! Issue tracking for Director.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Type of issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueType {
    /// Bug fix.
    Bug,
    /// New feature.
    Feature,
    /// Enhancement to existing feature.
    Enhancement,
    /// Technical debt/refactoring.
    Chore,
    /// Documentation.
    Documentation,
    /// Testing.
    Test,
    /// Research/investigation.
    Research,
}

impl Default for IssueType {
    fn default() -> Self {
        IssueType::Feature
    }
}

/// Priority level for an issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssuePriority {
    /// Trivial.
    Trivial,
    /// Low.
    Low,
    /// Medium.
    Medium,
    /// High.
    High,
    /// Blocker.
    Blocker,
}

impl Default for IssuePriority {
    fn default() -> Self {
        IssuePriority::Medium
    }
}

/// Issue status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    /// Backlog - not planned.
    Backlog,
    /// Todo - planned for work.
    Todo,
    /// In progress.
    InProgress,
    /// In review.
    InReview,
    /// Done.
    Done,
    /// Cancelled.
    Cancelled,
}

impl Default for IssueStatus {
    fn default() -> Self {
        IssueStatus::Backlog
    }
}

/// An issue/task in the project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    /// Unique identifier.
    pub id: Uuid,

    /// Issue type.
    pub issue_type: IssueType,

    /// Title.
    pub title: String,

    /// Description (markdown).
    pub description: Option<String>,

    /// Priority.
    pub priority: IssuePriority,

    /// Current status.
    pub status: IssueStatus,

    /// Related goal.
    pub goal_id: Option<Uuid>,

    /// Assignee (could be AI agent or human).
    pub assignee: Option<String>,

    /// Labels.
    pub labels: Vec<String>,

    /// Estimated effort (story points).
    pub estimate: Option<u8>,

    /// External issue ID (Plane.so, GitHub, etc.).
    pub external_id: Option<String>,

    /// External URL.
    pub external_url: Option<String>,

    /// Files to modify.
    pub affected_files: Vec<String>,

    /// Acceptance criteria.
    pub acceptance_criteria: Vec<String>,

    /// Created timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Updated timestamp.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Issue {
    /// Create a new issue.
    pub fn new(title: impl Into<String>, issue_type: IssueType) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            issue_type,
            title: title.into(),
            description: None,
            priority: IssuePriority::default(),
            status: IssueStatus::default(),
            goal_id: None,
            assignee: None,
            labels: vec![],
            estimate: None,
            external_id: None,
            external_url: None,
            affected_files: vec![],
            acceptance_criteria: vec![],
            created_at: now,
            updated_at: now,
        }
    }

    /// Create a bug issue.
    pub fn bug(title: impl Into<String>) -> Self {
        Self::new(title, IssueType::Bug)
    }

    /// Create a feature issue.
    pub fn feature(title: impl Into<String>) -> Self {
        Self::new(title, IssueType::Feature)
    }

    /// Create a chore issue.
    pub fn chore(title: impl Into<String>) -> Self {
        Self::new(title, IssueType::Chore)
    }

    /// Set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: IssuePriority) -> Self {
        self.priority = priority;
        self
    }

    /// Link to a goal.
    pub fn for_goal(mut self, goal_id: Uuid) -> Self {
        self.goal_id = Some(goal_id);
        self
    }

    /// Set assignee.
    pub fn assigned_to(mut self, assignee: impl Into<String>) -> Self {
        self.assignee = Some(assignee.into());
        self
    }

    /// Add a label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.labels.push(label.into());
        self
    }

    /// Set estimate.
    pub fn with_estimate(mut self, points: u8) -> Self {
        self.estimate = Some(points);
        self
    }

    /// Add affected file.
    pub fn affects_file(mut self, path: impl Into<String>) -> Self {
        self.affected_files.push(path.into());
        self
    }

    /// Add acceptance criteria.
    pub fn with_criteria(mut self, criteria: impl Into<String>) -> Self {
        self.acceptance_criteria.push(criteria.into());
        self
    }

    /// Link to external issue tracker.
    pub fn with_external(mut self, id: impl Into<String>, url: impl Into<String>) -> Self {
        self.external_id = Some(id.into());
        self.external_url = Some(url.into());
        self
    }

    /// Move to todo.
    pub fn plan(&mut self) {
        self.status = IssueStatus::Todo;
        self.updated_at = chrono::Utc::now();
    }

    /// Start work.
    pub fn start(&mut self) {
        self.status = IssueStatus::InProgress;
        self.updated_at = chrono::Utc::now();
    }

    /// Submit for review.
    pub fn review(&mut self) {
        self.status = IssueStatus::InReview;
        self.updated_at = chrono::Utc::now();
    }

    /// Complete.
    pub fn complete(&mut self) {
        self.status = IssueStatus::Done;
        self.updated_at = chrono::Utc::now();
    }

    /// Check if actionable.
    pub fn is_actionable(&self) -> bool {
        matches!(self.status, IssueStatus::Todo | IssueStatus::Backlog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_creation() {
        let issue = Issue::feature("Add login")
            .with_description("Add user authentication")
            .with_priority(IssuePriority::High)
            .with_estimate(5)
            .affects_file("src/auth.rs");

        assert_eq!(issue.title, "Add login");
        assert_eq!(issue.issue_type, IssueType::Feature);
        assert_eq!(issue.priority, IssuePriority::High);
        assert_eq!(issue.estimate, Some(5));
    }

    #[test]
    fn test_issue_workflow() {
        let mut issue = Issue::bug("Fix crash");

        assert_eq!(issue.status, IssueStatus::Backlog);

        issue.plan();
        assert_eq!(issue.status, IssueStatus::Todo);

        issue.start();
        assert_eq!(issue.status, IssueStatus::InProgress);

        issue.review();
        assert_eq!(issue.status, IssueStatus::InReview);

        issue.complete();
        assert_eq!(issue.status, IssueStatus::Done);
    }
}
