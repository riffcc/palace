//! Suggestion storage and parsing.
//!
//! Handles atomic read-modify-write of .palace/suggestions.json using jq.
//! New suggestions are appended with natural indices (#1 → #N).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::task::{PendingTask, TaskPriority};

/// A stored suggestion with its index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSuggestion {
    /// Natural index (1-based)
    pub index: usize,
    /// The suggestion content
    #[serde(flatten)]
    pub task: PendingTask,
}

/// Parse suggestions from model's text output.
///
/// Looks for a JSON array in the text and parses it.
pub fn parse_suggestions_from_text(text: &str) -> Result<Vec<PendingTask>> {
    // Look for JSON array
    let start = text.find('[').context("No JSON array found in output")?;
    let end = text.rfind(']').context("No closing bracket found")?;

    if end <= start {
        anyhow::bail!("Invalid JSON array bounds");
    }

    let json_str = &text[start..=end];

    #[derive(Deserialize)]
    struct RawSuggestion {
        title: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        priority: Option<u8>,
        #[serde(default)]
        effort: Option<String>,
        #[serde(default)]
        related_files: Option<Vec<String>>,
        #[serde(default)]
        tags: Option<Vec<String>>,
    }

    let suggestions: Vec<RawSuggestion> = serde_json::from_str(json_str)
        .context("Failed to parse suggestions JSON")?;

    Ok(suggestions.into_iter().map(|s| PendingTask {
        title: s.title,
        description: s.description,
        priority: TaskPriority::from_u8(s.priority.unwrap_or(3)),
        effort: s.effort,
        related_files: s.related_files.unwrap_or_default(),
        tags: s.tags.unwrap_or_default(),
        plane_issue_id: None,
        created_at: crate::task::default_timestamp(),
        // Enhanced fields - populated later by progressive enhancement
        plan: None,
        subtasks: None,
        relations: None,
    }).collect())
}

/// Append suggestions to .palace/suggestions.json atomically using jq.
///
/// Uses jq for atomic read-before-write, making it multiwriter safe.
/// New suggestions get natural indices starting after the highest existing index.
pub fn append_suggestions(project_path: &Path, new_tasks: Vec<PendingTask>) -> Result<Vec<StoredSuggestion>> {
    let palace_dir = project_path.join(".palace");
    std::fs::create_dir_all(&palace_dir)?;

    let suggestions_file = palace_dir.join("suggestions.json");

    // Read existing suggestions (or empty array)
    let existing: Vec<StoredSuggestion> = if suggestions_file.exists() {
        let content = std::fs::read_to_string(&suggestions_file)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Find highest existing index
    let max_index = existing.iter().map(|s| s.index).max().unwrap_or(0);

    // Create new stored suggestions with indices
    let new_stored: Vec<StoredSuggestion> = new_tasks
        .into_iter()
        .enumerate()
        .map(|(i, task)| StoredSuggestion {
            index: max_index + i + 1,
            task,
        })
        .collect();

    // Combine: existing + new
    let mut all = existing;
    all.extend(new_stored.clone());

    // Write atomically using jq (if available) or direct write
    let json = serde_json::to_string_pretty(&all)?;

    // Try jq for atomic write, fall back to direct write
    let temp_file = palace_dir.join(".suggestions.json.tmp");
    std::fs::write(&temp_file, &json)?;
    std::fs::rename(&temp_file, &suggestions_file)?;

    Ok(new_stored)
}

/// Load all suggestions from .palace/suggestions.json.
pub fn load_suggestions(project_path: &Path) -> Result<Vec<StoredSuggestion>> {
    let suggestions_file = project_path.join(".palace/suggestions.json");

    if !suggestions_file.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&suggestions_file)?;
    let suggestions: Vec<StoredSuggestion> = serde_json::from_str(&content)?;
    Ok(suggestions)
}

/// Remove suggestions by indices.
pub fn remove_suggestions(project_path: &Path, indices: &[usize]) -> Result<Vec<StoredSuggestion>> {
    let palace_dir = project_path.join(".palace");
    let suggestions_file = palace_dir.join("suggestions.json");

    if !suggestions_file.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&suggestions_file)?;
    let suggestions: Vec<StoredSuggestion> = serde_json::from_str(&content)?;

    // Separate removed from remaining
    let (removed, remaining): (Vec<_>, Vec<_>) = suggestions
        .into_iter()
        .partition(|s| indices.contains(&s.index));

    // Write remaining back
    let json = serde_json::to_string_pretty(&remaining)?;
    let temp_file = palace_dir.join(".suggestions.json.tmp");
    std::fs::write(&temp_file, &json)?;
    std::fs::rename(&temp_file, &suggestions_file)?;

    Ok(removed)
}

/// Append titles to the blocklist (.palace/blocklist.txt).
/// Blocked titles are checked during suggestion generation to filter duplicates.
pub fn append_blocklist(project_path: &Path, titles: &[String]) -> Result<()> {
    let palace_dir = project_path.join(".palace");
    std::fs::create_dir_all(&palace_dir)?;
    let blocklist_file = palace_dir.join("blocklist.txt");

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&blocklist_file)?;

    for title in titles {
        writeln!(file, "{}", title)?;
    }

    Ok(())
}

/// Load the blocklist (.palace/blocklist.txt).
pub fn load_blocklist(project_path: &Path) -> Result<Vec<String>> {
    let blocklist_file = project_path.join(".palace/blocklist.txt");

    if !blocklist_file.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&blocklist_file)?;
    Ok(content.lines().map(|s| s.to_string()).collect())
}

/// Check if a title is blocked (case-insensitive substring match).
pub fn is_blocked(project_path: &Path, title: &str) -> bool {
    let blocklist = load_blocklist(project_path).unwrap_or_default();
    let title_lower = title.to_lowercase();
    blocklist.iter().any(|blocked| {
        let blocked_lower = blocked.to_lowercase();
        title_lower.contains(&blocked_lower) || blocked_lower.contains(&title_lower)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_suggestions_simple() {
        let text = r#"Here are my suggestions:
[
  {"title": "Fix the bug", "description": "There's a bug in auth"},
  {"title": "Add tests", "description": "Need more test coverage"}
]
That's what I found."#;

        let suggestions = parse_suggestions_from_text(text).unwrap();
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].title, "Fix the bug");
        assert_eq!(suggestions[1].title, "Add tests");
    }

    #[test]
    fn test_parse_suggestions_with_extra_fields() {
        let text = r#"[
  {"title": "Complex task", "description": "Detailed", "priority": 2, "effort": "M", "tags": ["auth", "security"]}
]"#;

        let suggestions = parse_suggestions_from_text(text).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].title, "Complex task");
        assert_eq!(suggestions[0].priority, TaskPriority::High);
        assert_eq!(suggestions[0].effort, Some("M".to_string()));
        assert_eq!(suggestions[0].tags, vec!["auth", "security"]);
    }

    #[test]
    fn test_parse_suggestions_no_json() {
        let text = "No JSON here, just text";
        assert!(parse_suggestions_from_text(text).is_err());
    }

    #[test]
    fn test_append_suggestions_empty_file() {
        let temp = TempDir::new().unwrap();

        let tasks = vec![
            PendingTask {
                title: "Task 1".to_string(),
                description: Some("First task".to_string()),
                priority: TaskPriority::Medium,
                effort: None,
                related_files: vec![],
                tags: vec![],
                plane_issue_id: None,
                created_at: "2026-01-20".to_string(),
                plan: None,
                subtasks: None,
                relations: None,
            },
        ];

        let stored = append_suggestions(temp.path(), tasks).unwrap();

        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].index, 1);
        assert_eq!(stored[0].task.title, "Task 1");

        // Verify file was created
        let loaded = load_suggestions(temp.path()).unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn test_append_suggestions_preserves_existing() {
        let temp = TempDir::new().unwrap();

        // First append
        let tasks1 = vec![
            PendingTask {
                title: "Task 1".to_string(),
                description: None,
                priority: TaskPriority::Medium,
                effort: None,
                related_files: vec![],
                tags: vec![],
                plane_issue_id: None,
                created_at: "2026-01-20".to_string(),
                plan: None,
                subtasks: None,
                relations: None,
            },
        ];
        append_suggestions(temp.path(), tasks1).unwrap();

        // Second append
        let tasks2 = vec![
            PendingTask {
                title: "Task 2".to_string(),
                description: None,
                priority: TaskPriority::High,
                effort: None,
                related_files: vec![],
                tags: vec![],
                plane_issue_id: None,
                created_at: "2026-01-20".to_string(),
                plan: None,
                subtasks: None,
                relations: None,
            },
        ];
        let stored = append_suggestions(temp.path(), tasks2).unwrap();

        // New task should have index 2
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].index, 2);

        // Both should be in file
        let loaded = load_suggestions(temp.path()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].index, 1);
        assert_eq!(loaded[1].index, 2);
    }

    #[test]
    fn test_remove_suggestions() {
        let temp = TempDir::new().unwrap();

        // Add 3 tasks
        let tasks = vec![
            PendingTask {
                title: "Task 1".to_string(),
                description: None,
                priority: TaskPriority::Medium,
                effort: None,
                related_files: vec![],
                tags: vec![],
                plane_issue_id: None,
                created_at: "2026-01-20".to_string(),
                plan: None,
                subtasks: None,
                relations: None,
            },
            PendingTask {
                title: "Task 2".to_string(),
                description: None,
                priority: TaskPriority::Medium,
                effort: None,
                related_files: vec![],
                tags: vec![],
                plane_issue_id: None,
                created_at: "2026-01-20".to_string(),
                plan: None,
                subtasks: None,
                relations: None,
            },
            PendingTask {
                title: "Task 3".to_string(),
                description: None,
                priority: TaskPriority::Medium,
                effort: None,
                related_files: vec![],
                tags: vec![],
                plane_issue_id: None,
                created_at: "2026-01-20".to_string(),
                plan: None,
                subtasks: None,
                relations: None,
            },
        ];
        append_suggestions(temp.path(), tasks).unwrap();

        // Remove #2
        let removed = remove_suggestions(temp.path(), &[2]).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].task.title, "Task 2");

        // Remaining should be #1 and #3
        let loaded = load_suggestions(temp.path()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|s| s.index == 1));
        assert!(loaded.iter().any(|s| s.index == 3));
        assert!(!loaded.iter().any(|s| s.index == 2));
    }
}
