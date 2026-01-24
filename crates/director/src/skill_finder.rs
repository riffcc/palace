//! SkillFinder - automatic skill selection based on context.
//!
//! Uses a fast local model to analyze:
//! - Issue/task description
//! - Codebase context (file types, frameworks)
//! - Project conventions
//!
//! Returns recommended skills to load for optimal session performance.

use crate::{DirectorError, DirectorResult};
use llm_code_sdk::skills::LocalSkill;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A skill in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntry {
    /// Skill name/identifier.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Keywords that trigger this skill.
    pub keywords: Vec<String>,
    /// File extensions that trigger this skill.
    pub extensions: Vec<String>,
    /// Frameworks/libraries that trigger this skill.
    pub frameworks: Vec<String>,
    /// Path to the skill file (if local).
    pub path: Option<PathBuf>,
    /// Priority (higher = more likely to be selected).
    pub priority: i32,
}

impl SkillEntry {
    /// Create a new skill entry.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            keywords: Vec::new(),
            extensions: Vec::new(),
            frameworks: Vec::new(),
            path: None,
            priority: 0,
        }
    }

    /// Add keywords.
    pub fn with_keywords(mut self, keywords: &[&str]) -> Self {
        self.keywords = keywords.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Add file extensions.
    pub fn with_extensions(mut self, extensions: &[&str]) -> Self {
        self.extensions = extensions.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Add frameworks.
    pub fn with_frameworks(mut self, frameworks: &[&str]) -> Self {
        self.frameworks = frameworks.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Set path.
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }
}

/// Registry of available skills.
#[derive(Debug, Default)]
pub struct SkillRegistry {
    skills: HashMap<String, SkillEntry>,
}

impl SkillRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    /// Create a registry with built-in skills.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        // Language skills
        registry.register(
            SkillEntry::new("typescript", "TypeScript/JavaScript expert")
                .with_extensions(&["ts", "tsx", "js", "jsx", "mjs", "cjs"])
                .with_frameworks(&["react", "vue", "angular", "node", "deno", "bun"])
                .with_keywords(&["typescript", "javascript", "frontend", "web"])
                .with_priority(10),
        );

        registry.register(
            SkillEntry::new("rust", "Rust systems programming expert")
                .with_extensions(&["rs"])
                .with_frameworks(&["tokio", "axum", "actix", "warp", "rocket"])
                .with_keywords(&["rust", "cargo", "crate", "unsafe", "lifetime"])
                .with_priority(10),
        );

        registry.register(
            SkillEntry::new("python", "Python expert")
                .with_extensions(&["py", "pyi", "pyx"])
                .with_frameworks(&["django", "flask", "fastapi", "pytorch", "tensorflow"])
                .with_keywords(&["python", "pip", "virtualenv", "poetry"])
                .with_priority(10),
        );

        registry.register(
            SkillEntry::new("go", "Go/Golang expert")
                .with_extensions(&["go"])
                .with_frameworks(&["gin", "echo", "fiber", "grpc"])
                .with_keywords(&["golang", "go mod", "goroutine"])
                .with_priority(10),
        );

        registry.register(
            SkillEntry::new("perl", "Perl expert (including Proxmox patterns)")
                .with_extensions(&["pl", "pm", "t"])
                .with_frameworks(&["proxmox", "pve", "mojolicious", "dancer"])
                .with_keywords(&["perl", "cpan", "moose", "proxmox"])
                .with_priority(10),
        );

        // Framework skills
        registry.register(
            SkillEntry::new("vue", "Vue.js framework expert")
                .with_extensions(&["vue"])
                .with_frameworks(&["vue", "vuex", "pinia", "nuxt", "vite"])
                .with_keywords(&["vue", "composition api", "options api", "vuex", "pinia"])
                .with_priority(20),
        );

        registry.register(
            SkillEntry::new("react", "React framework expert")
                .with_frameworks(&["react", "redux", "next", "gatsby"])
                .with_keywords(&["react", "jsx", "hooks", "redux", "next.js"])
                .with_priority(20),
        );

        registry.register(
            SkillEntry::new("svelte", "Svelte framework expert")
                .with_extensions(&["svelte"])
                .with_frameworks(&["svelte", "sveltekit"])
                .with_keywords(&["svelte", "sveltekit", "runes"])
                .with_priority(20),
        );

        // Domain skills
        registry.register(
            SkillEntry::new("distributed-systems", "Distributed systems architect")
                .with_keywords(&[
                    "distributed", "consensus", "raft", "paxos", "cap theorem",
                    "eventual consistency", "replication", "sharding", "cluster"
                ])
                .with_priority(30),
        );

        registry.register(
            SkillEntry::new("testing", "Testing and QA expert")
                .with_keywords(&["test", "spec", "mock", "stub", "coverage", "e2e", "unit test"])
                .with_priority(15),
        );

        registry.register(
            SkillEntry::new("devops", "DevOps and infrastructure expert")
                .with_extensions(&["yml", "yaml", "tf", "hcl"])
                .with_frameworks(&["kubernetes", "docker", "terraform", "ansible"])
                .with_keywords(&["deploy", "ci/cd", "pipeline", "container", "k8s"])
                .with_priority(15),
        );

        registry
    }

    /// Register a skill.
    pub fn register(&mut self, skill: SkillEntry) {
        self.skills.insert(skill.name.clone(), skill);
    }

    /// Get a skill by name.
    pub fn get(&self, name: &str) -> Option<&SkillEntry> {
        self.skills.get(name)
    }

    /// List all skills.
    pub fn list(&self) -> Vec<&SkillEntry> {
        self.skills.values().collect()
    }

    /// Load skills from a directory.
    pub fn load_from_dir(&mut self, dir: &Path) -> std::io::Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                let name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                // Read first line as description
                let content = std::fs::read_to_string(&path)?;
                let description = content.lines().next().unwrap_or(&name).to_string();

                self.register(
                    SkillEntry::new(&name, description)
                        .with_path(&path)
                        .with_priority(5), // User skills have lower base priority
                );
            }
        }

        Ok(())
    }
}

/// Context for skill finding.
#[derive(Debug, Clone, Default)]
pub struct SkillContext {
    /// Issue/task description.
    pub task_description: String,
    /// File extensions found in the project.
    pub extensions: Vec<String>,
    /// Frameworks detected (from package.json, Cargo.toml, etc.).
    pub frameworks: Vec<String>,
    /// Additional keywords from the context.
    pub keywords: Vec<String>,
    /// Project name.
    pub project_name: Option<String>,
}

impl SkillContext {
    /// Create context from a project path.
    pub fn from_project(path: &Path) -> Self {
        let mut ctx = Self::default();

        // Scan for file extensions
        ctx.extensions = Self::scan_extensions(path);

        // Detect frameworks
        ctx.frameworks = Self::detect_frameworks(path);

        // Get project name
        ctx.project_name = path.file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        ctx
    }

    /// Scan directory for file extensions.
    fn scan_extensions(path: &Path) -> Vec<String> {
        let mut extensions = HashMap::new();

        fn walk(dir: &Path, extensions: &mut HashMap<String, usize>, depth: usize) {
            if depth > 5 {
                return; // Don't go too deep
            }

            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();

                    // Skip hidden and common ignore dirs
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with('.') || name == "node_modules" || name == "target" || name == "vendor" {
                            continue;
                        }
                    }

                    if path.is_dir() {
                        walk(&path, extensions, depth + 1);
                    } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        *extensions.entry(ext.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }

        walk(path, &mut extensions, 0);

        // Return extensions sorted by frequency
        let mut exts: Vec<_> = extensions.into_iter().collect();
        exts.sort_by(|a, b| b.1.cmp(&a.1));
        exts.into_iter().take(10).map(|(e, _)| e).collect()
    }

    /// Detect frameworks from manifest files.
    fn detect_frameworks(path: &Path) -> Vec<String> {
        let mut frameworks = Vec::new();

        // Check Cargo.toml
        let cargo_toml = path.join("Cargo.toml");
        if cargo_toml.exists() {
            if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
                for fw in ["tokio", "axum", "actix", "warp", "rocket", "serde", "clap"] {
                    if content.contains(fw) {
                        frameworks.push(fw.to_string());
                    }
                }
            }
        }

        // Check package.json
        let package_json = path.join("package.json");
        if package_json.exists() {
            if let Ok(content) = std::fs::read_to_string(&package_json) {
                for fw in ["react", "vue", "angular", "svelte", "next", "nuxt", "express", "nest"] {
                    if content.contains(fw) {
                        frameworks.push(fw.to_string());
                    }
                }
            }
        }

        // Check requirements.txt / pyproject.toml
        let requirements = path.join("requirements.txt");
        let pyproject = path.join("pyproject.toml");
        for py_file in [&requirements, &pyproject] {
            if py_file.exists() {
                if let Ok(content) = std::fs::read_to_string(py_file) {
                    for fw in ["django", "flask", "fastapi", "pytorch", "tensorflow", "numpy", "pandas"] {
                        if content.to_lowercase().contains(fw) {
                            frameworks.push(fw.to_string());
                        }
                    }
                }
            }
        }

        frameworks
    }

    /// Add task description.
    pub fn with_task(mut self, description: &str) -> Self {
        self.task_description = description.to_string();
        // Extract keywords from description
        let words: Vec<_> = description.to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .map(|s| s.to_string())
            .collect();
        self.keywords.extend(words);
        self
    }
}

/// SkillFinder - selects appropriate skills based on context.
pub struct SkillFinder {
    registry: SkillRegistry,
    /// LLM endpoint for smart skill selection (optional).
    llm_url: Option<String>,
    /// Model for skill selection.
    model: String,
}

impl SkillFinder {
    /// Create a new SkillFinder with built-in skills.
    pub fn new() -> Self {
        Self {
            registry: SkillRegistry::with_builtins(),
            llm_url: None,
            model: "nvidia_orchestrator-8b".to_string(), // Fast local model
        }
    }

    /// Enable LLM-based skill selection.
    pub fn with_llm(mut self, url: &str, model: &str) -> Self {
        self.llm_url = Some(url.to_string());
        self.model = model.to_string();
        self
    }

    /// Load additional skills from directories.
    pub fn load_skills_from(&mut self, dirs: &[&Path]) -> &mut Self {
        for dir in dirs {
            let _ = self.registry.load_from_dir(dir);
        }
        self
    }

    /// Find skills based on context using rule-based matching.
    pub fn find_rules(&self, context: &SkillContext) -> Vec<String> {
        let mut scores: HashMap<String, i32> = HashMap::new();

        for skill in self.registry.list() {
            let mut score = skill.priority;

            // Match extensions
            for ext in &context.extensions {
                if skill.extensions.contains(ext) {
                    score += 20;
                }
            }

            // Match frameworks
            for fw in &context.frameworks {
                if skill.frameworks.iter().any(|f| fw.to_lowercase().contains(&f.to_lowercase())) {
                    score += 30;
                }
            }

            // Match keywords in task description
            let task_lower = context.task_description.to_lowercase();
            for kw in &skill.keywords {
                if task_lower.contains(&kw.to_lowercase()) {
                    score += 15;
                }
            }

            // Match keywords from context
            for kw in &context.keywords {
                if skill.keywords.iter().any(|k| k.to_lowercase() == kw.to_lowercase()) {
                    score += 10;
                }
            }

            if score > skill.priority {
                scores.insert(skill.name.clone(), score);
            }
        }

        // Sort by score and return top skills
        let mut skills: Vec<_> = scores.into_iter().collect();
        skills.sort_by(|a, b| b.1.cmp(&a.1));

        skills.into_iter()
            .take(5) // Max 5 skills
            .filter(|(_, score)| *score >= 20) // Minimum threshold
            .map(|(name, _)| name)
            .collect()
    }

    /// Find skills using LLM (async).
    pub async fn find_llm(&self, context: &SkillContext) -> DirectorResult<Vec<String>> {
        let url = match &self.llm_url {
            Some(u) => u,
            None => return Ok(self.find_rules(context)), // Fallback to rules
        };

        // Build prompt
        let available_skills: Vec<_> = self.registry.list()
            .iter()
            .map(|s| format!("- {}: {}", s.name, s.description))
            .collect();

        let prompt = format!(
            r#"You are a skill selector for an AI coding agent.

Available skills:
{}

Project context:
- File extensions: {:?}
- Frameworks: {:?}
- Project: {:?}

Task: {}

Select 1-5 skills that would help with this task. Return ONLY a JSON array of skill names.
Example: ["typescript", "vue", "testing"]

Skills:"#,
            available_skills.join("\n"),
            context.extensions,
            context.frameworks,
            context.project_name,
            context.task_description
        );

        // Call LLM
        let client = reqwest::Client::new();
        let response = client.post(format!("{}/chat/completions", url))
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [{"role": "user", "content": prompt}],
                "max_tokens": 100,
                "temperature": 0.1
            }))
            .send()
            .await
            .map_err(|e| DirectorError::Other(format!("LLM request failed: {}", e)))?;

        let body: serde_json::Value = response.json().await
            .map_err(|e| DirectorError::Other(format!("Failed to parse LLM response: {}", e)))?;

        // Extract skills from response
        let content = body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("[]");

        // Try to parse as JSON array
        if let Ok(skills) = serde_json::from_str::<Vec<String>>(content.trim()) {
            // Validate skills exist
            let valid: Vec<_> = skills.into_iter()
                .filter(|s| self.registry.get(s).is_some())
                .collect();
            return Ok(valid);
        }

        // Fallback to rules if LLM parsing fails
        Ok(self.find_rules(context))
    }

    /// Find skills (uses LLM if available, otherwise rules).
    pub async fn find(&self, context: &SkillContext) -> Vec<String> {
        if self.llm_url.is_some() {
            self.find_llm(context).await.unwrap_or_else(|e| {
                tracing::warn!("LLM skill finding failed: {}, falling back to rules", e);
                self.find_rules(context)
            })
        } else {
            self.find_rules(context)
        }
    }

    /// Get the skill registry.
    pub fn registry(&self) -> &SkillRegistry {
        &self.registry
    }

    /// Get mutable registry.
    pub fn registry_mut(&mut self) -> &mut SkillRegistry {
        &mut self.registry
    }
}

impl Default for SkillFinder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_registry_builtins() {
        let registry = SkillRegistry::with_builtins();
        assert!(registry.get("typescript").is_some());
        assert!(registry.get("rust").is_some());
        assert!(registry.get("vue").is_some());
    }

    #[test]
    fn test_skill_finder_rules() {
        let finder = SkillFinder::new();

        let context = SkillContext {
            task_description: "Fix the TypeScript compilation error in the Vue component".to_string(),
            extensions: vec!["ts".to_string(), "vue".to_string()],
            frameworks: vec!["vue".to_string()],
            keywords: vec![],
            project_name: Some("my-app".to_string()),
        };

        let skills = finder.find_rules(&context);
        assert!(skills.contains(&"typescript".to_string()));
        assert!(skills.contains(&"vue".to_string()));
    }

    #[test]
    fn test_skill_context_from_project() {
        // This would need a real directory to test properly
        let ctx = SkillContext::default()
            .with_task("Implement the new authentication flow");

        assert!(ctx.task_description.contains("authentication"));
        assert!(ctx.keywords.len() > 0);
    }
}
