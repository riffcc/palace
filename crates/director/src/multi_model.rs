//! Multi-model scaffolding system.
//!
//! Enables multiple models to collaborate on a single change:
//!
//! 1. **Scaffolder** (fast model) creates structure with placeholders
//! 2. **Specialists** (capable models) fill placeholders
//! 3. **Validator** runs tests, reports failures
//! 4. **Iteration** until all tests pass
//!
//! ## Placeholder Syntax
//!
//! ```text
//! // @model:devstral: Implement the sorting algorithm
//! // @model:full: Add error handling for edge cases
//! // @model:cloud: Optimize for performance, consider cache invalidation
//! ```
//!
//! ## Example Flow
//!
//! ```ignore
//! let mut multi = MultiModelEdit::new(project_path);
//!
//! // Fast model scaffolds
//! multi.scaffold(ModelTier::Fast, "Create UserService with CRUD methods").await;
//!
//! // Devstral fills implementation
//! multi.fill_placeholders(ModelTier::Code).await;
//!
//! // Full model handles edge cases
//! multi.fill_placeholders(ModelTier::Full).await;
//!
//! // Validate and commit
//! if multi.validate().await.passed {
//!     multi.commit()?;
//! }
//! ```

use crate::{DirectorError, DirectorResult, ModelTier, ModelLadder, ModelEndpoint};
use llm_code_sdk::skills::SkillStack;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A placeholder in code for a specific model to fill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placeholder {
    /// File containing the placeholder.
    pub file: PathBuf,
    /// Line number (1-indexed).
    pub line: usize,
    /// Target model tier.
    pub target_tier: ModelTier,
    /// Description/prompt for the model.
    pub prompt: String,
    /// Context lines around the placeholder.
    pub context: String,
    /// Whether this placeholder has been filled.
    pub filled: bool,
    /// The content that was filled in.
    pub filled_content: Option<String>,
}

impl Placeholder {
    /// Parse a placeholder from a comment line.
    pub fn parse(file: &Path, line: usize, comment: &str, context: &str) -> Option<Self> {
        // Pattern: // @model:<tier>: <prompt>
        let re = Regex::new(r"@model:(\w+):\s*(.+)").ok()?;
        let caps = re.captures(comment)?;

        let tier_str = caps.get(1)?.as_str();
        let prompt = caps.get(2)?.as_str().to_string();

        let target_tier = tier_str.parse().ok()?;

        Some(Self {
            file: file.to_path_buf(),
            line,
            target_tier,
            prompt,
            context: context.to_string(),
            filled: false,
            filled_content: None,
        })
    }

    /// Format as a prompt for the model.
    pub fn to_prompt(&self) -> String {
        format!(
            "Fill in the placeholder in {}:{}.\n\n\
            Context:\n```\n{}\n```\n\n\
            Instruction: {}\n\n\
            Provide ONLY the code to replace the placeholder comment. No explanation.",
            self.file.display(),
            self.line,
            self.context,
            self.prompt
        )
    }
}

/// Result of scanning for placeholders.
#[derive(Debug, Clone, Default)]
pub struct PlaceholderScan {
    /// All placeholders found.
    pub placeholders: Vec<Placeholder>,
    /// Count by tier.
    pub by_tier: HashMap<ModelTier, usize>,
    /// Files scanned.
    pub files_scanned: usize,
}

impl PlaceholderScan {
    /// Get placeholders for a specific tier.
    pub fn for_tier(&self, tier: ModelTier) -> Vec<&Placeholder> {
        self.placeholders.iter()
            .filter(|p| p.target_tier == tier && !p.filled)
            .collect()
    }

    /// Get unfilled count.
    pub fn unfilled_count(&self) -> usize {
        self.placeholders.iter().filter(|p| !p.filled).count()
    }
}

/// Multi-model edit orchestrator.
pub struct MultiModelEdit {
    /// Project root.
    project_path: PathBuf,
    /// Model ladder for endpoint selection.
    ladder: ModelLadder,
    /// Current placeholders.
    placeholders: Vec<Placeholder>,
    /// Shadow files (path -> content).
    shadow_files: HashMap<PathBuf, String>,
    /// Skills to apply.
    skills: SkillStack,
    /// Whether scaffolding is complete.
    scaffolded: bool,
    /// Edit transaction for atomic commit.
    transaction_name: String,
}

impl MultiModelEdit {
    /// Create a new multi-model edit.
    pub fn new(project_path: impl Into<PathBuf>) -> Self {
        Self {
            project_path: project_path.into(),
            ladder: ModelLadder::new(),
            placeholders: Vec::new(),
            shadow_files: HashMap::new(),
            skills: SkillStack::new(),
            scaffolded: false,
            transaction_name: format!("multi-edit-{}", chrono::Utc::now().timestamp()),
        }
    }

    /// Set the model ladder.
    pub fn with_ladder(mut self, ladder: ModelLadder) -> Self {
        self.ladder = ladder;
        self
    }

    /// Add skills.
    pub fn with_skills(mut self, skills: SkillStack) -> Self {
        self.skills = skills;
        self
    }

    /// Scaffold the change using a fast model.
    ///
    /// The scaffolder creates the structure with `// @model:<tier>:` placeholders
    /// for more capable models to fill in.
    pub async fn scaffold(&mut self, task: &str) -> DirectorResult<()> {
        let endpoint = self.ladder.endpoint_for(ModelTier::Fast)
            .or_else(|| self.ladder.endpoint_for(ModelTier::Flash))
            .ok_or_else(|| DirectorError::Other("No scaffolding model available".into()))?
            .clone();

        // Build scaffolding prompt
        let prompt = format!(
            r#"You are a code scaffolder. Create the structure for the following task,
using placeholder comments for complex implementation details.

TASK: {}

PLACEHOLDER SYNTAX:
- `// @model:code: <description>` - For implementation code (Devstral)
- `// @model:full: <description>` - For complex logic (GLM-4.7)
- `// @model:cloud: <description>` - For critical/optimized code (GPT-5.2/Opus)

Create the file structure and scaffolding. Use placeholders liberally -
another model will fill them in. Focus on:
1. Correct structure and architecture
2. Type definitions and interfaces
3. Function signatures
4. Clear placeholder descriptions

Example:
```rust
pub struct UserService {{
    db: Database,
}}

impl UserService {{
    pub fn new(db: Database) -> Self {{
        Self {{ db }}
    }}

    // @model:code: Implement user creation with validation
    pub fn create_user(&self, name: &str, email: &str) -> Result<User, Error> {{
        todo!()
    }}

    // @model:full: Implement complex permission checking with role hierarchy
    pub fn check_permissions(&self, user: &User, action: &str) -> bool {{
        todo!()
    }}
}}
```

{}

Now create the scaffolding:"#,
            task,
            self.skills.to_system_prompt()
        );

        // Call the scaffolding model
        let response = self.call_model(&endpoint, &prompt).await?;

        // Parse response and extract files
        self.parse_scaffold_response(&response)?;

        // Scan for placeholders
        self.scan_placeholders();

        self.scaffolded = true;
        Ok(())
    }

    /// Call a model endpoint.
    async fn call_model(&self, endpoint: &ModelEndpoint, prompt: &str) -> DirectorResult<String> {
        let client = reqwest::Client::new();

        let mut req = client.post(format!("{}/chat/completions", endpoint.url))
            .json(&serde_json::json!({
                "model": endpoint.model,
                "messages": [{"role": "user", "content": prompt}],
                "max_tokens": 4096,
                "temperature": 0.3
            }));

        // Add API key if required
        if let Some(env_var) = &endpoint.api_key_env {
            if let Ok(key) = std::env::var(env_var) {
                req = req.bearer_auth(key);
            }
        }

        let response = req.send().await
            .map_err(|e| DirectorError::Other(format!("Model request failed: {}", e)))?;

        let body: serde_json::Value = response.json().await
            .map_err(|e| DirectorError::Other(format!("Failed to parse response: {}", e)))?;

        body["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| DirectorError::Other("No content in response".into()))
    }

    /// Parse scaffold response and extract files.
    fn parse_scaffold_response(&mut self, response: &str) -> DirectorResult<()> {
        // Extract code blocks with filenames
        let re = Regex::new(r"```(\w+)?\s*(?://\s*)?(\S+\.\w+)?\n([\s\S]*?)```")
            .map_err(|e| DirectorError::Other(e.to_string()))?;

        for caps in re.captures_iter(response) {
            let filename = caps.get(2).map(|m| m.as_str());
            let content = caps.get(3).map(|m| m.as_str()).unwrap_or("");

            if let Some(name) = filename {
                let path = self.project_path.join(name);
                self.shadow_files.insert(path, content.to_string());
            }
        }

        // If no files extracted, try to find inline code
        if self.shadow_files.is_empty() {
            // Look for obvious file markers
            let lines: Vec<&str> = response.lines().collect();
            let mut current_file: Option<PathBuf> = None;
            let mut current_content = String::new();

            for line in lines {
                if line.starts_with("// File:") || line.starts_with("# File:") {
                    if let Some(file) = current_file.take() {
                        self.shadow_files.insert(file, current_content.clone());
                    }
                    let name = line.trim_start_matches("// File:")
                        .trim_start_matches("# File:")
                        .trim();
                    current_file = Some(self.project_path.join(name));
                    current_content.clear();
                } else if current_file.is_some() {
                    current_content.push_str(line);
                    current_content.push('\n');
                }
            }

            if let Some(file) = current_file {
                self.shadow_files.insert(file, current_content);
            }
        }

        Ok(())
    }

    /// Scan shadow files for placeholders.
    fn scan_placeholders(&mut self) {
        self.placeholders.clear();

        for (path, content) in &self.shadow_files {
            let lines: Vec<&str> = content.lines().collect();

            for (i, line) in lines.iter().enumerate() {
                if line.contains("@model:") {
                    // Get context (5 lines before and after)
                    let start = i.saturating_sub(5);
                    let end = (i + 6).min(lines.len());
                    let context = lines[start..end].join("\n");

                    if let Some(placeholder) = Placeholder::parse(path, i + 1, line, &context) {
                        self.placeholders.push(placeholder);
                    }
                }
            }
        }
    }

    /// Get current placeholder scan.
    pub fn placeholder_scan(&self) -> PlaceholderScan {
        let mut by_tier: HashMap<ModelTier, usize> = HashMap::new();

        for p in &self.placeholders {
            *by_tier.entry(p.target_tier).or_insert(0) += 1;
        }

        PlaceholderScan {
            placeholders: self.placeholders.clone(),
            by_tier,
            files_scanned: self.shadow_files.len(),
        }
    }

    /// Fill placeholders for a specific tier.
    pub async fn fill_placeholders(&mut self, tier: ModelTier) -> DirectorResult<usize> {
        let endpoint = self.ladder.endpoint_for(tier)
            .ok_or_else(|| DirectorError::Other(format!("No endpoint for tier {:?}", tier)))?
            .clone();

        // Collect indices of placeholders to fill
        let indices: Vec<usize> = self.placeholders.iter()
            .enumerate()
            .filter(|(_, p)| p.target_tier == tier && !p.filled)
            .map(|(i, _)| i)
            .collect();

        let mut filled = 0;

        for idx in indices {
            let prompt = self.placeholders[idx].to_prompt();
            let file = self.placeholders[idx].file.clone();
            let line = self.placeholders[idx].line;

            match self.call_model(&endpoint, &prompt).await {
                Ok(response) => {
                    // Extract code from response (strip markdown if present)
                    let code = self.extract_code(&response);

                    // Update shadow file
                    if let Some(content) = self.shadow_files.get(&file).cloned() {
                        let new_content = self.replace_placeholder(&content, line, &code);
                        self.shadow_files.insert(file, new_content);
                    }

                    self.placeholders[idx].filled = true;
                    self.placeholders[idx].filled_content = Some(code);
                    filled += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to fill placeholder at {}:{}: {}",
                        self.placeholders[idx].file.display(),
                        self.placeholders[idx].line,
                        e
                    );
                }
            }
        }

        // Re-scan for any new placeholders (models might add their own)
        self.scan_placeholders();

        Ok(filled)
    }

    /// Extract code from a model response.
    fn extract_code(&self, response: &str) -> String {
        // Try to extract from code block
        let re = Regex::new(r"```\w*\n([\s\S]*?)```").unwrap();
        if let Some(caps) = re.captures(response) {
            return caps.get(1).map(|m| m.as_str()).unwrap_or(response).to_string();
        }

        // Otherwise return as-is
        response.to_string()
    }

    /// Replace a placeholder line with new content.
    fn replace_placeholder(&self, content: &str, line: usize, new_code: &str) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let mut result = String::new();

        for (i, l) in lines.iter().enumerate() {
            if i + 1 == line {
                // Replace placeholder with new code
                result.push_str(new_code);
                if !new_code.ends_with('\n') {
                    result.push('\n');
                }
            } else {
                result.push_str(l);
                result.push('\n');
            }
        }

        result
    }

    /// Fill all placeholders in tier order.
    pub async fn fill_all(&mut self) -> DirectorResult<usize> {
        let mut total = 0;

        for tier in ModelTier::all() {
            let count = self.fill_placeholders(*tier).await?;
            if count > 0 {
                tracing::info!("Filled {} placeholders with {:?} tier", count, tier);
            }
            total += count;
        }

        Ok(total)
    }

    /// Validate the changes (compile + tests).
    pub async fn validate(&self) -> DirectorResult<ValidationResult> {
        // Create temporary directory
        let temp_dir = tempfile::tempdir()
            .map_err(|e| DirectorError::Other(format!("Failed to create temp dir: {}", e)))?;

        // Copy project to temp
        self.copy_to_temp(temp_dir.path())?;

        // Apply shadow files
        for (path, content) in &self.shadow_files {
            if let Ok(rel) = path.strip_prefix(&self.project_path) {
                let dest = temp_dir.path().join(rel);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(&dest, content)
                    .map_err(|e| DirectorError::Other(format!("Failed to write: {}", e)))?;
            }
        }

        // Run cargo check
        let check = std::process::Command::new("cargo")
            .arg("check")
            .current_dir(temp_dir.path())
            .output()
            .map_err(|e| DirectorError::Other(format!("cargo check failed: {}", e)))?;

        if !check.status.success() {
            let stderr = String::from_utf8_lossy(&check.stderr);
            return Ok(ValidationResult {
                passed: false,
                compile_errors: stderr.lines()
                    .filter(|l| l.contains("error"))
                    .map(|l| l.to_string())
                    .collect(),
                test_failures: vec![],
                unfilled_placeholders: self.placeholders.iter().filter(|p| !p.filled).count(),
            });
        }

        // Run tests
        let test = std::process::Command::new("cargo")
            .arg("test")
            .current_dir(temp_dir.path())
            .output()
            .map_err(|e| DirectorError::Other(format!("cargo test failed: {}", e)))?;

        let test_failures = if !test.status.success() {
            let stdout = String::from_utf8_lossy(&test.stdout);
            stdout.lines()
                .filter(|l| l.contains("FAILED"))
                .map(|l| l.to_string())
                .collect()
        } else {
            vec![]
        };

        Ok(ValidationResult {
            passed: check.status.success() && test.status.success(),
            compile_errors: vec![],
            test_failures,
            unfilled_placeholders: self.placeholders.iter().filter(|p| !p.filled).count(),
        })
    }

    /// Copy project to temp directory.
    fn copy_to_temp(&self, dest: &Path) -> DirectorResult<()> {
        // Copy essential files
        for file in ["Cargo.toml", "Cargo.lock"] {
            let src = self.project_path.join(file);
            if src.exists() {
                std::fs::copy(&src, dest.join(file)).ok();
            }
        }

        // Copy src and crates
        for dir in ["src", "crates"] {
            let src_dir = self.project_path.join(dir);
            if src_dir.exists() {
                copy_dir_all(&src_dir, &dest.join(dir))
                    .map_err(|e| DirectorError::Other(format!("Failed to copy: {}", e)))?;
            }
        }

        Ok(())
    }

    /// Commit all changes.
    pub fn commit(&self) -> DirectorResult<()> {
        for (path, content) in &self.shadow_files {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(path, content)
                .map_err(|e| DirectorError::Other(format!("Failed to write {:?}: {}", path, e)))?;
        }

        Ok(())
    }

    /// Get shadow files.
    pub fn shadow_files(&self) -> &HashMap<PathBuf, String> {
        &self.shadow_files
    }

    /// Check if scaffolding is complete.
    pub fn is_scaffolded(&self) -> bool {
        self.scaffolded
    }
}

/// Validation result.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether validation passed.
    pub passed: bool,
    /// Compilation errors.
    pub compile_errors: Vec<String>,
    /// Test failures.
    pub test_failures: Vec<String>,
    /// Unfilled placeholder count.
    pub unfilled_placeholders: usize,
}

impl ValidationResult {
    /// Get summary.
    pub fn summary(&self) -> String {
        if self.passed {
            "✅ All checks passed".to_string()
        } else {
            let mut parts = vec![];
            if !self.compile_errors.is_empty() {
                parts.push(format!("{} compile errors", self.compile_errors.len()));
            }
            if !self.test_failures.is_empty() {
                parts.push(format!("{} test failures", self.test_failures.len()));
            }
            if self.unfilled_placeholders > 0 {
                parts.push(format!("{} unfilled placeholders", self.unfilled_placeholders));
            }
            format!("❌ {}", parts.join(", "))
        }
    }
}

/// Recursively copy a directory.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());

        if ty.is_dir() {
            // Skip target directory
            if entry.file_name() == "target" {
                continue;
            }
            copy_dir_all(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder_parse() {
        let placeholder = Placeholder::parse(
            Path::new("src/lib.rs"),
            10,
            "// @model:code: Implement the sorting algorithm",
            "fn sort() { todo!() }",
        );

        assert!(placeholder.is_some());
        let p = placeholder.unwrap();
        assert_eq!(p.target_tier, ModelTier::Code);
        assert!(p.prompt.contains("sorting"));
    }

    #[test]
    fn test_multi_model_edit_new() {
        let edit = MultiModelEdit::new("/tmp/test");
        assert!(!edit.is_scaffolded());
        assert!(edit.shadow_files().is_empty());
    }
}
