//! Performance profiler for model effectiveness.
//!
//! Tracks real-world metrics for each model:
//! - **Effectiveness**: Did edits pass tests on first try?
//! - **Quality**: Clean code, no lint issues, performant?
//! - **Durability**: How many commits before code is overwritten?
//!
//! Uses ReDB for persistent local storage at `~/.palace/profiler.db`.

use crate::{DirectorError, DirectorResult, ModelTier};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Table for edit records.
const EDITS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("edits");

/// Table for model stats.
const MODEL_STATS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("model_stats");

/// Table for durability tracking (file hash -> edit info).
const DURABILITY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("durability");

/// Table for skill stats.
const SKILL_STATS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("skill_stats");

/// Table for model+skill combo stats.
const COMBO_STATS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("combo_stats");

/// An edit record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditRecord {
    /// Unique edit ID.
    pub id: String,
    /// Model that made the edit.
    pub model: String,
    /// Model tier.
    pub tier: ModelTier,
    /// Skills active during this edit.
    pub skills: Vec<String>,
    /// File path edited.
    pub file: String,
    /// Function/symbol edited (if known).
    pub symbol: Option<String>,
    /// Session ID.
    pub session_id: Option<String>,
    /// Timestamp.
    pub timestamp: u64,
    /// Whether edit passed tests on first try.
    pub first_try_success: bool,
    /// Number of attempts before success.
    pub attempts: u32,
    /// Whether edit had lint issues.
    pub had_lint_issues: bool,
    /// Lines of code added.
    pub lines_added: u32,
    /// Lines of code removed.
    pub lines_removed: u32,
    /// Complexity score (if computed).
    pub complexity: Option<f32>,
    /// Git commit hash after edit.
    pub commit_hash: Option<String>,
    /// Hash of the symbol content (from SmartRead AST).
    pub symbol_hash: Option<String>,
    /// Whether this edit has been overwritten.
    pub overwritten: bool,
    /// Commit where this was overwritten (if known).
    pub overwritten_at: Option<String>,
    /// Number of commits this edit lasted.
    pub commits_survived: Option<u32>,
}

/// Hash a symbol's content for tracking.
pub fn hash_symbol_content(content: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Normalize whitespace for consistent hashing
    let normalized: String = content.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Skill effectiveness statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillStats {
    /// Skill name.
    pub skill: String,
    /// Total edits with this skill.
    pub total_edits: u64,
    /// First-try successes.
    pub first_try_successes: u64,
    /// Lint issues.
    pub lint_issues: u64,
    /// Overwrites.
    pub overwrites: u64,
    /// Avg commits survived.
    pub avg_commits_survived: f32,
    /// Effectiveness score.
    pub effectiveness_score: f32,
    /// Quality score.
    pub quality_score: f32,
    /// Durability score.
    pub durability_score: f32,
    /// Overall score.
    pub overall_score: f32,
    /// Models this skill works best with.
    pub best_models: Vec<String>,
    /// Last updated.
    pub updated_at: u64,
}

impl SkillStats {
    pub fn new(skill: &str) -> Self {
        Self {
            skill: skill.to_string(),
            updated_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            ..Default::default()
        }
    }

    pub fn recalculate(&mut self) {
        self.effectiveness_score = if self.total_edits > 0 {
            (self.first_try_successes as f32 / self.total_edits as f32) * 100.0
        } else { 0.0 };

        self.quality_score = if self.total_edits > 0 {
            ((self.total_edits - self.lint_issues) as f32 / self.total_edits as f32) * 100.0
        } else { 0.0 };

        self.durability_score = (self.avg_commits_survived / 10.0 * 100.0).min(100.0);

        self.overall_score = self.effectiveness_score * 0.4
            + self.quality_score * 0.3
            + self.durability_score * 0.3;

        self.updated_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    }

    pub fn update_from_edit(&mut self, edit: &EditRecord) {
        self.total_edits += 1;
        if edit.first_try_success { self.first_try_successes += 1; }
        if edit.had_lint_issues { self.lint_issues += 1; }
        if edit.overwritten { self.overwrites += 1; }
        if let Some(survived) = edit.commits_survived {
            let total = self.avg_commits_survived * (self.overwrites.saturating_sub(1)) as f32;
            self.avg_commits_survived = (total + survived as f32) / self.overwrites.max(1) as f32;
        }
        self.recalculate();
    }
}

/// Model+Skill combo statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComboStats {
    /// Model name.
    pub model: String,
    /// Skill name.
    pub skill: String,
    /// Combo key (model:skill).
    pub key: String,
    /// Total edits.
    pub total_edits: u64,
    /// First-try successes.
    pub first_try_successes: u64,
    /// Effectiveness score.
    pub effectiveness_score: f32,
    /// Overall score.
    pub overall_score: f32,
    /// Last updated.
    pub updated_at: u64,
}

impl ComboStats {
    pub fn new(model: &str, skill: &str) -> Self {
        Self {
            model: model.to_string(),
            skill: skill.to_string(),
            key: format!("{}:{}", model, skill),
            updated_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            ..Default::default()
        }
    }

    pub fn update_from_edit(&mut self, edit: &EditRecord) {
        self.total_edits += 1;
        if edit.first_try_success { self.first_try_successes += 1; }
        self.effectiveness_score = if self.total_edits > 0 {
            (self.first_try_successes as f32 / self.total_edits as f32) * 100.0
        } else { 0.0 };
        self.overall_score = self.effectiveness_score; // Simplified for combos
        self.updated_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    }
}

impl EditRecord {
    /// Create a new edit record.
    pub fn new(model: &str, tier: ModelTier, file: &str) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            id,
            model: model.to_string(),
            tier,
            skills: Vec::new(),
            file: file.to_string(),
            symbol: None,
            session_id: None,
            timestamp,
            first_try_success: false,
            attempts: 0,
            had_lint_issues: false,
            lines_added: 0,
            lines_removed: 0,
            complexity: None,
            commit_hash: None,
            symbol_hash: None,
            overwritten: false,
            overwritten_at: None,
            commits_survived: None,
        }
    }

    /// Set skills.
    pub fn with_skills(mut self, skills: Vec<String>) -> Self {
        self.skills = skills;
        self
    }

    /// Set symbol hash from content.
    pub fn with_symbol_content(mut self, content: &str) -> Self {
        self.symbol_hash = Some(hash_symbol_content(content));
        self
    }

    /// Set symbol hash directly.
    pub fn with_symbol_hash(mut self, hash: &str) -> Self {
        self.symbol_hash = Some(hash.to_string());
        self
    }

    /// Mark as successful on first try.
    pub fn success_first_try(mut self) -> Self {
        self.first_try_success = true;
        self.attempts = 1;
        self
    }

    /// Set attempts.
    pub fn with_attempts(mut self, attempts: u32) -> Self {
        self.attempts = attempts;
        self.first_try_success = attempts == 1;
        self
    }

    /// Set session.
    pub fn with_session(mut self, session_id: &str) -> Self {
        self.session_id = Some(session_id.to_string());
        self
    }

    /// Set symbol.
    pub fn with_symbol(mut self, symbol: &str) -> Self {
        self.symbol = Some(symbol.to_string());
        self
    }

    /// Set line counts.
    pub fn with_lines(mut self, added: u32, removed: u32) -> Self {
        self.lines_added = added;
        self.lines_removed = removed;
        self
    }

    /// Set commit hash.
    pub fn with_commit(mut self, hash: &str) -> Self {
        self.commit_hash = Some(hash.to_string());
        self
    }

    /// Mark as having lint issues.
    pub fn with_lint_issues(mut self) -> Self {
        self.had_lint_issues = true;
        self
    }
}

/// Aggregated model statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelStats {
    /// Model identifier.
    pub model: String,
    /// Model tier.
    pub tier: ModelTier,
    /// Total edits made.
    pub total_edits: u64,
    /// Edits that passed on first try.
    pub first_try_successes: u64,
    /// Edits that had lint issues.
    pub lint_issues: u64,
    /// Total attempts across all edits.
    pub total_attempts: u64,
    /// Total lines added.
    pub total_lines_added: u64,
    /// Total lines removed.
    pub total_lines_removed: u64,
    /// Edits that have been overwritten.
    pub overwrites: u64,
    /// Average commits survived before overwrite.
    pub avg_commits_survived: f32,
    /// Effectiveness score (0-100).
    pub effectiveness_score: f32,
    /// Quality score (0-100).
    pub quality_score: f32,
    /// Durability score (0-100).
    pub durability_score: f32,
    /// Overall score (weighted average).
    pub overall_score: f32,
    /// Last updated timestamp.
    pub updated_at: u64,
}

impl ModelStats {
    /// Create new stats for a model.
    pub fn new(model: &str, tier: ModelTier) -> Self {
        Self {
            model: model.to_string(),
            tier,
            updated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            ..Default::default()
        }
    }

    /// Recalculate scores.
    pub fn recalculate(&mut self) {
        // Effectiveness: % of first-try successes
        self.effectiveness_score = if self.total_edits > 0 {
            (self.first_try_successes as f32 / self.total_edits as f32) * 100.0
        } else {
            0.0
        };

        // Quality: inverse of lint issues
        self.quality_score = if self.total_edits > 0 {
            ((self.total_edits - self.lint_issues) as f32 / self.total_edits as f32) * 100.0
        } else {
            0.0
        };

        // Durability: based on avg commits survived (normalize to 10 commits = 100%)
        self.durability_score = (self.avg_commits_survived / 10.0 * 100.0).min(100.0);

        // Overall: weighted average (effectiveness 40%, quality 30%, durability 30%)
        self.overall_score = self.effectiveness_score * 0.4
            + self.quality_score * 0.3
            + self.durability_score * 0.3;

        self.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    /// Update from an edit record.
    pub fn update_from_edit(&mut self, edit: &EditRecord) {
        self.total_edits += 1;
        if edit.first_try_success {
            self.first_try_successes += 1;
        }
        if edit.had_lint_issues {
            self.lint_issues += 1;
        }
        self.total_attempts += edit.attempts as u64;
        self.total_lines_added += edit.lines_added as u64;
        self.total_lines_removed += edit.lines_removed as u64;

        if edit.overwritten {
            self.overwrites += 1;
        }

        // Update avg commits survived
        if let Some(survived) = edit.commits_survived {
            let total_survived = self.avg_commits_survived * (self.overwrites.saturating_sub(1)) as f32;
            self.avg_commits_survived = (total_survived + survived as f32) / self.overwrites.max(1) as f32;
        }

        self.recalculate();
    }
}

/// Profiler database.
pub struct ProfilerDB {
    db: Database,
    path: PathBuf,
}

impl ProfilerDB {
    /// Open or create the profiler database.
    pub fn open() -> DirectorResult<Self> {
        let path = Self::default_path();
        Self::open_at(&path)
    }

    /// Open at a specific path.
    pub fn open_at(path: &PathBuf) -> DirectorResult<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DirectorError::Io(e))?;
        }

        let db = Database::create(path)
            .map_err(|e| DirectorError::Other(format!("Failed to open profiler db: {}", e)))?;

        // Initialize tables
        let write_txn = db.begin_write()
            .map_err(|e| DirectorError::Other(format!("Failed to begin txn: {}", e)))?;
        {
            let _ = write_txn.open_table(EDITS_TABLE);
            let _ = write_txn.open_table(MODEL_STATS_TABLE);
            let _ = write_txn.open_table(DURABILITY_TABLE);
        }
        write_txn.commit()
            .map_err(|e| DirectorError::Other(format!("Failed to commit: {}", e)))?;

        Ok(Self {
            db,
            path: path.clone(),
        })
    }

    /// Default database path.
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".palace/profiler.db")
    }

    /// Record an edit.
    pub fn record_edit(&self, edit: &EditRecord) -> DirectorResult<()> {
        let write_txn = self.db.begin_write()
            .map_err(|e| DirectorError::Other(format!("Failed to begin txn: {}", e)))?;

        {
            let mut table = write_txn.open_table(EDITS_TABLE)
                .map_err(|e| DirectorError::Other(format!("Failed to open table: {}", e)))?;

            let data = serde_json::to_vec(edit)
                .map_err(|e| DirectorError::Other(format!("Failed to serialize: {}", e)))?;

            table.insert(edit.id.as_str(), data.as_slice())
                .map_err(|e| DirectorError::Other(format!("Failed to insert: {}", e)))?;
        }

        write_txn.commit()
            .map_err(|e| DirectorError::Other(format!("Failed to commit: {}", e)))?;

        // Update model stats
        self.update_model_stats(edit)?;

        Ok(())
    }

    /// Update model statistics.
    fn update_model_stats(&self, edit: &EditRecord) -> DirectorResult<()> {
        let mut stats = self.get_model_stats(&edit.model)?
            .unwrap_or_else(|| ModelStats::new(&edit.model, edit.tier));

        stats.update_from_edit(edit);

        let write_txn = self.db.begin_write()
            .map_err(|e| DirectorError::Other(format!("Failed to begin txn: {}", e)))?;

        {
            let mut table = write_txn.open_table(MODEL_STATS_TABLE)
                .map_err(|e| DirectorError::Other(format!("Failed to open table: {}", e)))?;

            let data = serde_json::to_vec(&stats)
                .map_err(|e| DirectorError::Other(format!("Failed to serialize: {}", e)))?;

            table.insert(edit.model.as_str(), data.as_slice())
                .map_err(|e| DirectorError::Other(format!("Failed to insert: {}", e)))?;
        }

        write_txn.commit()
            .map_err(|e| DirectorError::Other(format!("Failed to commit: {}", e)))?;

        Ok(())
    }

    /// Get model statistics.
    pub fn get_model_stats(&self, model: &str) -> DirectorResult<Option<ModelStats>> {
        let read_txn = self.db.begin_read()
            .map_err(|e| DirectorError::Other(format!("Failed to begin txn: {}", e)))?;

        let table = read_txn.open_table(MODEL_STATS_TABLE)
            .map_err(|e| DirectorError::Other(format!("Failed to open table: {}", e)))?;

        match table.get(model) {
            Ok(Some(data)) => {
                let stats: ModelStats = serde_json::from_slice(data.value())
                    .map_err(|e| DirectorError::Other(format!("Failed to deserialize: {}", e)))?;
                Ok(Some(stats))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(DirectorError::Other(format!("Failed to get: {}", e))),
        }
    }

    /// Get all model stats, ranked by overall score.
    pub fn get_all_model_stats(&self) -> DirectorResult<Vec<ModelStats>> {
        let read_txn = self.db.begin_read()
            .map_err(|e| DirectorError::Other(format!("Failed to begin txn: {}", e)))?;

        let table = read_txn.open_table(MODEL_STATS_TABLE)
            .map_err(|e| DirectorError::Other(format!("Failed to open table: {}", e)))?;

        let mut all_stats = Vec::new();

        for result in table.iter().map_err(|e| DirectorError::Other(format!("Failed to iter: {}", e)))? {
            let (_, data) = result.map_err(|e| DirectorError::Other(format!("Failed to read: {}", e)))?;
            let stats: ModelStats = serde_json::from_slice(data.value())
                .map_err(|e| DirectorError::Other(format!("Failed to deserialize: {}", e)))?;
            all_stats.push(stats);
        }

        // Sort by overall score descending
        all_stats.sort_by(|a, b| b.overall_score.partial_cmp(&a.overall_score).unwrap_or(std::cmp::Ordering::Equal));

        Ok(all_stats)
    }

    /// Get stats for a specific tier.
    pub fn get_tier_stats(&self, tier: ModelTier) -> DirectorResult<Vec<ModelStats>> {
        let all = self.get_all_model_stats()?;
        Ok(all.into_iter().filter(|s| s.tier == tier).collect())
    }

    /// Get the best model for a tier.
    pub fn best_model_for_tier(&self, tier: ModelTier) -> DirectorResult<Option<ModelStats>> {
        let tier_stats = self.get_tier_stats(tier)?;
        Ok(tier_stats.into_iter().next())
    }

    /// Mark an edit as overwritten.
    pub fn mark_overwritten(&self, edit_id: &str, at_commit: &str, commits_survived: u32) -> DirectorResult<()> {
        // First, read the current edit
        let current_edit: Option<EditRecord> = {
            let read_txn = self.db.begin_read()
                .map_err(|e| DirectorError::Other(format!("Failed to begin read txn: {}", e)))?;

            let table = read_txn.open_table(EDITS_TABLE)
                .map_err(|e| DirectorError::Other(format!("Failed to open table: {}", e)))?;

            match table.get(edit_id) {
                Ok(Some(data)) => {
                    let edit: EditRecord = serde_json::from_slice(data.value())
                        .map_err(|e| DirectorError::Other(format!("Failed to deserialize: {}", e)))?;
                    Some(edit)
                }
                Ok(None) => None,
                Err(e) => return Err(DirectorError::Other(format!("Failed to get: {}", e))),
            }
        };

        // Update if found
        if let Some(mut edit) = current_edit {
            edit.overwritten = true;
            edit.overwritten_at = Some(at_commit.to_string());
            edit.commits_survived = Some(commits_survived);

            // Write updated edit
            let write_txn = self.db.begin_write()
                .map_err(|e| DirectorError::Other(format!("Failed to begin write txn: {}", e)))?;

            {
                let mut table = write_txn.open_table(EDITS_TABLE)
                    .map_err(|e| DirectorError::Other(format!("Failed to open table: {}", e)))?;

                let new_data = serde_json::to_vec(&edit)
                    .map_err(|e| DirectorError::Other(format!("Failed to serialize: {}", e)))?;

                table.insert(edit_id, new_data.as_slice())
                    .map_err(|e| DirectorError::Other(format!("Failed to insert: {}", e)))?;
            }

            write_txn.commit()
                .map_err(|e| DirectorError::Other(format!("Failed to commit: {}", e)))?;

            // Update model stats
            self.update_model_stats(&edit)?;
        }

        Ok(())
    }

    /// Get recent edits.
    pub fn recent_edits(&self, limit: usize) -> DirectorResult<Vec<EditRecord>> {
        let read_txn = self.db.begin_read()
            .map_err(|e| DirectorError::Other(format!("Failed to begin txn: {}", e)))?;

        let table = read_txn.open_table(EDITS_TABLE)
            .map_err(|e| DirectorError::Other(format!("Failed to open table: {}", e)))?;

        let mut edits = Vec::new();

        for result in table.iter().map_err(|e| DirectorError::Other(format!("Failed to iter: {}", e)))? {
            let (_, data) = result.map_err(|e| DirectorError::Other(format!("Failed to read: {}", e)))?;
            let edit: EditRecord = serde_json::from_slice(data.value())
                .map_err(|e| DirectorError::Other(format!("Failed to deserialize: {}", e)))?;
            edits.push(edit);
        }

        // Sort by timestamp descending and take limit
        edits.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        edits.truncate(limit);

        Ok(edits)
    }

    /// Generate a summary report.
    pub fn summary_report(&self) -> DirectorResult<ProfilerReport> {
        let all_stats = self.get_all_model_stats()?;
        let recent = self.recent_edits(100)?;

        let total_edits: u64 = all_stats.iter().map(|s| s.total_edits).sum();
        let total_first_try: u64 = all_stats.iter().map(|s| s.first_try_successes).sum();
        let total_overwrites: u64 = all_stats.iter().map(|s| s.overwrites).sum();

        let overall_effectiveness = if total_edits > 0 {
            (total_first_try as f32 / total_edits as f32) * 100.0
        } else {
            0.0
        };

        let avg_durability = if !all_stats.is_empty() {
            all_stats.iter().map(|s| s.avg_commits_survived).sum::<f32>() / all_stats.len() as f32
        } else {
            0.0
        };

        Ok(ProfilerReport {
            total_edits,
            total_first_try_successes: total_first_try,
            total_overwrites,
            overall_effectiveness,
            avg_durability_commits: avg_durability,
            model_count: all_stats.len(),
            top_models: all_stats.into_iter().take(5).collect(),
            recent_edits: recent,
        })
    }
}

/// Summary report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilerReport {
    pub total_edits: u64,
    pub total_first_try_successes: u64,
    pub total_overwrites: u64,
    pub overall_effectiveness: f32,
    pub avg_durability_commits: f32,
    pub model_count: usize,
    pub top_models: Vec<ModelStats>,
    pub recent_edits: Vec<EditRecord>,
}

impl ProfilerReport {
    /// Format as markdown.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str("# Palace Profiler Report\n\n");

        md.push_str("## Overview\n\n");
        md.push_str(&format!("- **Total Edits:** {}\n", self.total_edits));
        md.push_str(&format!("- **First-Try Successes:** {} ({:.1}%)\n",
            self.total_first_try_successes, self.overall_effectiveness));
        md.push_str(&format!("- **Overwrites:** {}\n", self.total_overwrites));
        md.push_str(&format!("- **Avg Durability:** {:.1} commits\n", self.avg_durability_commits));
        md.push_str(&format!("- **Models Tracked:** {}\n\n", self.model_count));

        md.push_str("## Top Models\n\n");
        md.push_str("| Model | Tier | Edits | Effectiveness | Quality | Durability | Overall |\n");
        md.push_str("|-------|------|-------|---------------|---------|------------|--------|\n");

        for model in &self.top_models {
            md.push_str(&format!(
                "| {} | {:?} | {} | {:.1}% | {:.1}% | {:.1}% | {:.1}% |\n",
                model.model, model.tier, model.total_edits,
                model.effectiveness_score, model.quality_score,
                model.durability_score, model.overall_score
            ));
        }

        md.push_str("\n## Recent Edits\n\n");
        for edit in self.recent_edits.iter().take(10) {
            let status = if edit.first_try_success { "✅" } else { "🔄" };
            md.push_str(&format!(
                "- {} `{}` edited `{}` ({} attempts)\n",
                status, edit.model, edit.file, edit.attempts
            ));
        }

        md
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_edit_record() {
        let edit = EditRecord::new("devstral-2", ModelTier::Code, "src/lib.rs")
            .success_first_try()
            .with_lines(50, 20)
            .with_symbol("handle_request");

        assert!(edit.first_try_success);
        assert_eq!(edit.attempts, 1);
        assert_eq!(edit.lines_added, 50);
    }

    #[test]
    fn test_model_stats() {
        let mut stats = ModelStats::new("test-model", ModelTier::Fast);

        let edit1 = EditRecord::new("test-model", ModelTier::Fast, "a.rs").success_first_try();
        let edit2 = EditRecord::new("test-model", ModelTier::Fast, "b.rs").with_attempts(3);

        stats.update_from_edit(&edit1);
        stats.update_from_edit(&edit2);

        assert_eq!(stats.total_edits, 2);
        assert_eq!(stats.first_try_successes, 1);
        assert_eq!(stats.effectiveness_score, 50.0);
    }

    #[test]
    fn test_profiler_db() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test-profiler.db");

        let db = ProfilerDB::open_at(&path).unwrap();

        let edit = EditRecord::new("test-model", ModelTier::Fast, "test.rs")
            .success_first_try();

        db.record_edit(&edit).unwrap();

        let stats = db.get_model_stats("test-model").unwrap();
        assert!(stats.is_some());
        assert_eq!(stats.unwrap().total_edits, 1);
    }
}
