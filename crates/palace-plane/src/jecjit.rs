//! JECJIT: Just Enough Context Just In Time
//!
//! Surfaces relevant Plane.so context during code operations.
//! Never floods. Always relevant. Exactly what's needed.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::api::PlaneIssue;
use crate::config::ProjectConfig;

/// Cached issue with code correlations.
#[derive(Debug, Clone)]
pub struct CachedIssue {
    pub id: String,
    pub sequence_id: u32,
    pub name: String,
    pub description: Option<String>,
    pub state: String,
    pub priority: String,
    /// Files mentioned in this issue
    pub related_files: Vec<String>,
    /// Functions/symbols mentioned
    pub related_symbols: Vec<String>,
}

impl From<PlaneIssue> for CachedIssue {
    fn from(issue: PlaneIssue) -> Self {
        // Extract file references from description
        let (files, symbols) = extract_code_references(
            issue.description_html.as_deref().unwrap_or("")
        );

        Self {
            id: issue.id,
            sequence_id: issue.sequence_id,
            name: issue.name,
            description: issue.description_html,
            state: issue.state.unwrap_or_default(),
            priority: issue.priority.unwrap_or_else(|| "none".to_string()),
            related_files: files,
            related_symbols: symbols,
        }
    }
}

/// A spec item extracted from spec files.
#[derive(Debug, Clone)]
pub struct SpecItem {
    pub description: String,
    pub source: String,
}

/// JECJIT context provider.
pub struct JecjitContext {
    /// Cached issues by ID
    issues: Arc<RwLock<HashMap<String, CachedIssue>>>,
    /// File path → issue IDs
    file_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Symbol name → issue IDs
    symbol_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Spec items (parsed from spec_files)
    spec_items: Arc<RwLock<Vec<SpecItem>>>,
    /// Project config
    config: ProjectConfig,
}

impl JecjitContext {
    /// Create a new JECJIT context for a project.
    pub fn new(config: ProjectConfig) -> Self {
        Self {
            issues: Arc::new(RwLock::new(HashMap::new())),
            file_index: Arc::new(RwLock::new(HashMap::new())),
            symbol_index: Arc::new(RwLock::new(HashMap::new())),
            spec_items: Arc::new(RwLock::new(Vec::new())),
            config,
        }
    }

    /// Parse and cache spec items from configured spec_files.
    pub fn load_specs(&self, project_path: &std::path::Path) -> usize {
        let mut items = self.spec_items.write().unwrap();
        items.clear();

        for spec_file in &self.config.spec_files {
            let path = project_path.join(spec_file);
            if let Ok(content) = std::fs::read_to_string(&path) {
                let parsed = parse_spec_items(&content, spec_file);
                items.extend(parsed);
            }
        }

        items.len()
    }

    /// Get spec gaps (items with no matching issue).
    pub fn spec_gaps(&self) -> Vec<SpecItem> {
        let items = self.spec_items.read().unwrap();
        let issues = self.issues.read().unwrap();

        items.iter()
            .filter(|item| !has_matching_issue(&item.description, &issues))
            .cloned()
            .collect()
    }

    /// Format spec gaps for inline display.
    pub fn format_gaps(gaps: &[SpecItem]) -> String {
        if gaps.is_empty() {
            return String::new();
        }

        let mut output = String::from("\n### Spec Gaps\n");
        for gap in gaps.iter().take(3) {
            output.push_str(&format!("- {}\n", gap.description));
        }
        if gaps.len() > 3 {
            output.push_str(&format!("  (+{} more)\n", gaps.len() - 3));
        }
        output
    }

    /// Load/refresh issues from Plane.so.
    pub async fn refresh(&self) -> anyhow::Result<usize> {
        let client = crate::api::PlaneClient::new()?;
        let issues = client.list_active_issues(&self.config).await?;

        let mut cache = self.issues.write().unwrap();
        let mut file_idx = self.file_index.write().unwrap();
        let mut sym_idx = self.symbol_index.write().unwrap();

        cache.clear();
        file_idx.clear();
        sym_idx.clear();

        for issue in issues {
            let cached: CachedIssue = issue.into();
            let id = cached.id.clone();

            // Index by files
            for file in &cached.related_files {
                file_idx.entry(file.clone())
                    .or_default()
                    .push(id.clone());
            }

            // Index by symbols
            for sym in &cached.related_symbols {
                sym_idx.entry(sym.clone())
                    .or_default()
                    .push(id.clone());
            }

            cache.insert(id, cached);
        }

        Ok(cache.len())
    }

    /// Get context for a file path.
    /// Returns issues related to this file.
    pub fn context_for_file(&self, path: &str) -> Vec<IssueContext> {
        let file_idx = self.file_index.read().unwrap();
        let cache = self.issues.read().unwrap();

        // Normalize path
        let normalized = normalize_path(path);

        // Find matching issues
        let mut results = Vec::new();

        for (indexed_path, issue_ids) in file_idx.iter() {
            if paths_match(&normalized, indexed_path) {
                for id in issue_ids {
                    if let Some(issue) = cache.get(id) {
                        results.push(IssueContext {
                            id: format!("{}-{}", self.config.project_slug.to_uppercase(), issue.sequence_id),
                            name: issue.name.clone(),
                            state: issue.state.clone(),
                            priority: issue.priority.clone(),
                            relevance: "file".to_string(),
                        });
                    }
                }
            }
        }

        results
    }

    /// Get context for a symbol (function/type name).
    pub fn context_for_symbol(&self, symbol: &str) -> Vec<IssueContext> {
        let sym_idx = self.symbol_index.read().unwrap();
        let cache = self.issues.read().unwrap();

        let mut results = Vec::new();

        if let Some(issue_ids) = sym_idx.get(symbol) {
            for id in issue_ids {
                if let Some(issue) = cache.get(id) {
                    results.push(IssueContext {
                        id: format!("{}-{}", self.config.project_slug.to_uppercase(), issue.sequence_id),
                        name: issue.name.clone(),
                        state: issue.state.clone(),
                        priority: issue.priority.clone(),
                        relevance: "symbol".to_string(),
                    });
                }
            }
        }

        results
    }

    /// Search issues by text.
    pub fn search(&self, query: &str) -> Vec<IssueContext> {
        let cache = self.issues.read().unwrap();
        let query_lower = query.to_lowercase();

        cache.values()
            .filter(|issue| {
                issue.name.to_lowercase().contains(&query_lower) ||
                issue.description.as_ref()
                    .map(|d| d.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
            })
            .map(|issue| IssueContext {
                id: format!("{}-{}", self.config.project_slug.to_uppercase(), issue.sequence_id),
                name: issue.name.clone(),
                state: issue.state.clone(),
                priority: issue.priority.clone(),
                relevance: "search".to_string(),
            })
            .collect()
    }

    pub fn format_context(issues: &[IssueContext]) -> String {
        if issues.is_empty() {
            return String::new();
        }

        let mut output = String::from("\n### Related Issues\n");
        for issue in issues.iter().take(5) {
            output.push_str(&format!("- {}: {}\n", issue.id, issue.name));
        }
        if issues.len() > 5 {
            output.push_str(&format!("  (+{} more)\n", issues.len() - 5));
        }
        output
    }
}

/// Compact issue context for display.
#[derive(Debug, Clone)]
pub struct IssueContext {
    pub id: String,
    pub name: String,
    pub state: String,
    pub priority: String,
    pub relevance: String,
}

/// Extract code references (files and symbols) from text/HTML.
fn extract_code_references(text: &str) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut symbols = Vec::new();

    // Match file paths: src/foo/bar.rs, lib/something.py, etc.
    // Handles: backticks, HTML <code> tags, or plain text
    let file_pattern = regex::Regex::new(
        r"(?:^|[`\s\(>])([a-zA-Z0-9_\-./]+\.(rs|py|js|ts|tsx|go|c|cpp|h|hpp|java|rb))"
    ).ok();

    if let Some(re) = file_pattern {
        for cap in re.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                files.push(m.as_str().to_string());
            }
        }
    }

    // Match symbols in backticks: `foo_bar`, `MyStruct`, etc.
    let backtick_pattern = regex::Regex::new(r"`([a-zA-Z_][a-zA-Z0-9_]*)`").ok();
    if let Some(re) = backtick_pattern {
        for cap in re.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                let sym = m.as_str();
                if sym.len() > 2 && !is_common_word(sym) {
                    symbols.push(sym.to_string());
                }
            }
        }
    }

    // Match symbols in HTML <code> tags: <code>foo_bar</code>
    let code_tag_pattern = regex::Regex::new(r"<code>([a-zA-Z_][a-zA-Z0-9_]*)</code>").ok();
    if let Some(re) = code_tag_pattern {
        for cap in re.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                let sym = m.as_str();
                if sym.len() > 2 && !is_common_word(sym) && !symbols.contains(&sym.to_string()) {
                    symbols.push(sym.to_string());
                }
            }
        }
    }

    (files, symbols)
}

fn is_common_word(s: &str) -> bool {
    matches!(s.to_lowercase().as_str(),
        "the" | "and" | "for" | "with" | "this" | "that" | "from" | "into" |
        "true" | "false" | "none" | "some" | "null" | "undefined"
    )
}

fn normalize_path(path: &str) -> String {
    path.trim_start_matches("./")
        .trim_start_matches("/")
        .to_string()
}

fn paths_match(a: &str, b: &str) -> bool {
    let a_norm = normalize_path(a);
    let b_norm = normalize_path(b);

    a_norm == b_norm ||
    a_norm.ends_with(&b_norm) ||
    b_norm.ends_with(&a_norm)
}

/// Parse spec items from markdown content.
fn parse_spec_items(content: &str, source: &str) -> Vec<SpecItem> {
    let mut items = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Parse checklist items: - [ ] or - [x]
        if let Some(rest) = trimmed.strip_prefix("- [ ]").or_else(|| trimmed.strip_prefix("- [x]")) {
            let desc = rest.trim();
            if desc.len() > 3 {
                items.push(SpecItem {
                    description: desc.to_string(),
                    source: source.to_string(),
                });
            }
        }
        // Parse bullet items under relevant headers
        else if trimmed.starts_with("- ") && !trimmed.contains("[") {
            let desc = trimmed[2..].trim();
            if desc.len() > 5 && looks_like_task(desc) {
                items.push(SpecItem {
                    description: desc.to_string(),
                    source: source.to_string(),
                });
            }
        }
    }

    items
}

/// Check if text looks like a task/feature description.
fn looks_like_task(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.starts_with("add ") ||
    lower.starts_with("implement ") ||
    lower.starts_with("fix ") ||
    lower.starts_with("create ") ||
    lower.starts_with("build ") ||
    lower.starts_with("wire ") ||
    lower.starts_with("support ") ||
    lower.contains(" feature") ||
    lower.contains(" integration") ||
    lower.contains(" tool")
}

/// Check if a spec item has a matching Plane issue.
fn has_matching_issue(description: &str, issues: &HashMap<String, CachedIssue>) -> bool {
    let desc_lower = description.to_lowercase();
    let desc_words: Vec<&str> = desc_lower.split_whitespace()
        .filter(|w| w.len() > 3)
        .collect();

    for issue in issues.values() {
        let name_lower = issue.name.to_lowercase();

        // Exact substring match
        if name_lower.contains(&desc_lower) || desc_lower.contains(&name_lower) {
            return true;
        }

        // Word overlap match (3+ significant words)
        let name_words: Vec<&str> = name_lower.split_whitespace()
            .filter(|w| w.len() > 3)
            .collect();

        let overlap = desc_words.iter()
            .filter(|w| name_words.contains(w))
            .count();

        if overlap >= 2 {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_code_references() {
        let text = "Fix the bug in `authenticate` function in src/auth/login.rs";
        let (files, symbols) = extract_code_references(text);

        assert!(files.contains(&"src/auth/login.rs".to_string()));
        assert!(symbols.contains(&"authenticate".to_string()));
    }

    #[test]
    fn test_paths_match() {
        assert!(paths_match("src/foo.rs", "src/foo.rs"));
        assert!(paths_match("./src/foo.rs", "src/foo.rs"));
        assert!(paths_match("foo.rs", "src/foo.rs"));
    }
}
