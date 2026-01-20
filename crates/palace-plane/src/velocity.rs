//! Velocity tracking: Learn issue completion times per type×codebase.
//!
//! Tracks how long issues take to complete based on:
//! - Issue type (bug, docs, chore, refactor, feat, feat-XL)
//! - Codebase complexity (lines, dependencies, test coverage)
//!
//! Over time, this enables accurate auto-timeboxing for cycles.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Issue types for velocity tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueType {
    Bug,
    Docs,
    Chore,
    Refactor,
    Feat,
    FeatXL,
    /// Custom/discovered type
    #[serde(other)]
    Other,
}

impl IssueType {
    pub fn from_label(label: &str) -> Self {
        match label.to_lowercase().as_str() {
            "bug" => Self::Bug,
            "docs" | "documentation" => Self::Docs,
            "chore" | "maintenance" => Self::Chore,
            "refactor" | "refactoring" => Self::Refactor,
            "feat" | "feature" => Self::Feat,
            "feat-xl" | "feature-xl" | "epic" => Self::FeatXL,
            _ => Self::Other,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Docs => "docs",
            Self::Chore => "chore",
            Self::Refactor => "refactor",
            Self::Feat => "feat",
            Self::FeatXL => "feat-XL",
            Self::Other => "other",
        }
    }
}

/// A completed issue sample for velocity learning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VelocitySample {
    pub issue_id: String,
    pub issue_type: IssueType,
    pub duration_hours: f64,
    pub complexity_score: f64,
    pub completed_at: chrono::DateTime<chrono::Utc>,
}

/// Velocity statistics for an issue type.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VelocityStats {
    pub samples: u32,
    pub avg_hours: f64,
    pub std_dev: f64,
    pub min_hours: f64,
    pub max_hours: f64,
}

impl VelocityStats {
    fn update(&mut self, hours: f64) {
        self.samples += 1;

        if self.samples == 1 {
            self.avg_hours = hours;
            self.min_hours = hours;
            self.max_hours = hours;
            self.std_dev = 0.0;
        } else {
            // Welford's online algorithm for mean and variance
            let delta = hours - self.avg_hours;
            self.avg_hours += delta / self.samples as f64;
            let delta2 = hours - self.avg_hours;
            let m2 = self.std_dev.powi(2) * (self.samples - 1) as f64 + delta * delta2;
            self.std_dev = (m2 / self.samples as f64).sqrt();

            self.min_hours = self.min_hours.min(hours);
            self.max_hours = self.max_hours.max(hours);
        }
    }
}

/// Velocity tracker for a project.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VelocityTracker {
    /// Stats per issue type
    pub by_type: HashMap<IssueType, VelocityStats>,
    /// Recent samples (for learning)
    pub recent_samples: Vec<VelocitySample>,
    /// Project complexity baseline
    pub complexity_baseline: f64,
}

impl VelocityTracker {
    /// Load velocity data from project's .palace directory.
    pub fn load(project_path: &Path) -> Result<Self> {
        let velocity_path = project_path.join(".palace").join("velocity.json");

        if velocity_path.exists() {
            let content = std::fs::read_to_string(&velocity_path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }

    /// Save velocity data.
    pub fn save(&self, project_path: &Path) -> Result<()> {
        let palace_dir = project_path.join(".palace");
        std::fs::create_dir_all(&palace_dir)?;

        let velocity_path = palace_dir.join("velocity.json");
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&velocity_path, content)?;

        Ok(())
    }

    /// Record a completed issue.
    pub fn record_completion(
        &mut self,
        issue_id: &str,
        issue_type: IssueType,
        started_at: chrono::DateTime<chrono::Utc>,
        completed_at: chrono::DateTime<chrono::Utc>,
        complexity_score: f64,
    ) {
        let duration = completed_at - started_at;
        let hours = duration.num_minutes() as f64 / 60.0;

        // Update type stats
        self.by_type
            .entry(issue_type)
            .or_default()
            .update(hours);

        // Store sample
        self.recent_samples.push(VelocitySample {
            issue_id: issue_id.to_string(),
            issue_type,
            duration_hours: hours,
            complexity_score,
            completed_at,
        });

        // Keep only last 100 samples
        if self.recent_samples.len() > 100 {
            self.recent_samples.remove(0);
        }
    }

    /// Estimate time for an issue type.
    pub fn estimate_hours(&self, issue_type: IssueType) -> Option<f64> {
        self.by_type.get(&issue_type).map(|s| s.avg_hours)
    }

    /// Estimate time range (optimistic to pessimistic).
    pub fn estimate_range(&self, issue_type: IssueType) -> Option<(f64, f64, f64)> {
        self.by_type.get(&issue_type).map(|s| {
            let optimistic = (s.avg_hours - s.std_dev).max(s.min_hours);
            let pessimistic = s.avg_hours + s.std_dev * 2.0;
            (optimistic, s.avg_hours, pessimistic)
        })
    }

    /// Calculate suggested cycle timebox based on issues.
    pub fn suggest_cycle_timebox(&self, issue_types: &[IssueType]) -> Duration {
        let total_hours: f64 = issue_types
            .iter()
            .filter_map(|t| self.estimate_hours(*t))
            .sum();

        // Add 20% buffer
        let buffered_hours = total_hours * 1.2;

        // Default to 8 hours if no data
        let hours = if buffered_hours > 0.0 { buffered_hours } else { 8.0 };

        Duration::from_secs((hours * 3600.0) as u64)
    }

    /// Format stats for display.
    pub fn format_stats(&self) -> String {
        let mut output = String::from("## Velocity Stats\n");

        for (issue_type, stats) in &self.by_type {
            output.push_str(&format!(
                "- {}: {:.1}h avg ({} samples, σ={:.1}h)\n",
                issue_type.as_str(),
                stats.avg_hours,
                stats.samples,
                stats.std_dev
            ));
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_velocity_tracking() {
        let mut tracker = VelocityTracker::default();

        let now = chrono::Utc::now();
        let two_hours_ago = now - chrono::Duration::hours(2);

        tracker.record_completion(
            "test-1",
            IssueType::Bug,
            two_hours_ago,
            now,
            1.0,
        );

        assert_eq!(tracker.estimate_hours(IssueType::Bug), Some(2.0));
    }

    #[test]
    fn test_issue_type_from_label() {
        assert_eq!(IssueType::from_label("bug"), IssueType::Bug);
        assert_eq!(IssueType::from_label("FEAT-XL"), IssueType::FeatXL);
        assert_eq!(IssueType::from_label("unknown"), IssueType::Other);
    }
}
