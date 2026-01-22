//! Task storage layer for Palace.
//!
//! Tasks are stored in <project>/.palace/
//! - PENDING-{id}.json - pending suggestions
//! - APPROVED-{id}.json - approved tasks (linked to Plane.so)
//!
//! The .palace directory should be added to .gitignore.

use crate::task::PendingTask;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Task storage manager.
pub struct TaskStorage {
    storage_dir: PathBuf,
}

impl TaskStorage {
    /// Create storage for a project.
    ///
    /// Storage is in `<project>/.palace/` - add to .gitignore.
    pub fn new(project_path: &Path) -> Result<Self> {
        let canonical = project_path.canonicalize()
            .context("Failed to resolve project path")?;

        let storage_dir = canonical.join(".palace");
        std::fs::create_dir_all(&storage_dir)
            .context("Failed to create .palace directory")?;

        Ok(Self { storage_dir })
    }

    /// Get next available pending ID.
    fn next_pending_id(&self) -> Result<u32> {
        let mut max_id = 0u32;

        for entry in std::fs::read_dir(&self.storage_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if name.starts_with("PENDING-") && name.ends_with(".json") {
                if let Some(id_str) = name.strip_prefix("PENDING-").and_then(|s| s.strip_suffix(".json")) {
                    if let Ok(id) = id_str.parse::<u32>() {
                        max_id = max_id.max(id);
                    }
                }
            }
        }

        Ok(max_id + 1)
    }

    /// Store pending tasks, returns assigned IDs.
    pub fn store_pending(&self, tasks: &[PendingTask]) -> Result<Vec<u32>> {
        let mut ids = Vec::with_capacity(tasks.len());
        let mut next_id = self.next_pending_id()?;

        for task in tasks {
            let id = next_id;
            next_id += 1;

            let path = self.storage_dir.join(format!("PENDING-{}.json", id));
            let content = serde_json::to_string_pretty(task)?;
            std::fs::write(&path, content)?;

            ids.push(id);
        }

        Ok(ids)
    }

    /// List all pending tasks, sorted by ID (newest first).
    pub fn list_pending(&self) -> Result<Vec<(u32, PendingTask)>> {
        let mut tasks = Vec::new();

        for entry in std::fs::read_dir(&self.storage_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if name.starts_with("PENDING-") && name.ends_with(".json") {
                if let Some(id_str) = name.strip_prefix("PENDING-").and_then(|s| s.strip_suffix(".json")) {
                    if let Ok(id) = id_str.parse::<u32>() {
                        let content = std::fs::read_to_string(entry.path())?;
                        let task: PendingTask = serde_json::from_str(&content)?;
                        tasks.push((id, task));
                    }
                }
            }
        }

        // Sort by ID descending (newest first)
        tasks.sort_by(|a, b| b.0.cmp(&a.0));

        Ok(tasks)
    }

    /// Get pending tasks by display indices (1-indexed).
    pub fn get_pending_by_indices(&self, indices: &[usize]) -> Result<Vec<(u32, PendingTask)>> {
        let all_tasks = self.list_pending()?;
        let mut result = Vec::new();

        for &idx in indices {
            if idx == 0 || idx > all_tasks.len() {
                continue;
            }
            result.push(all_tasks[idx - 1].clone());
        }

        Ok(result)
    }

    /// Remove pending tasks by display indices (1-indexed).
    pub fn remove_pending(&self, indices: &[usize]) -> Result<Vec<(u32, PendingTask)>> {
        let all_tasks = self.list_pending()?;
        let mut removed = Vec::new();

        for &idx in indices {
            if idx == 0 || idx > all_tasks.len() {
                continue;
            }

            let (id, task) = &all_tasks[idx - 1];
            let path = self.storage_dir.join(format!("PENDING-{}.json", id));

            if path.exists() {
                std::fs::remove_file(&path)?;
                removed.push((*id, task.clone()));
            }
        }

        Ok(removed)
    }

    /// Mark a pending task as approved.
    pub fn mark_approved(&self, pending_id: u32, plane_issue_id: &str) -> Result<()> {
        let old_path = self.storage_dir.join(format!("PENDING-{}.json", pending_id));

        if old_path.exists() {
            let content = std::fs::read_to_string(&old_path)?;
            let mut task: PendingTask = serde_json::from_str(&content)?;
            task.plane_issue_id = Some(plane_issue_id.to_string());

            let new_path = self.storage_dir.join(format!("APPROVED-{}.json", pending_id));
            let new_content = serde_json::to_string_pretty(&task)?;
            std::fs::write(&new_path, new_content)?;

            std::fs::remove_file(&old_path)?;
        }

        Ok(())
    }
}
