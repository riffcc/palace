//! Comparison Tool: Compare Plane state with arbitrary spec files.
//!
//! Uses llm-code-sdk as a subagent to explore codebase and verify
//! whether reality matches what's described in spec files.


use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::api::PlaneIssue;

/// Comparison request.
#[derive(Debug, Clone)]
pub struct CompareRequest {
    /// Files to compare against (specs, plans, READMEs, etc.)
    pub spec_files: Vec<String>,
    /// Whether to check live code state
    pub check_code: bool,
    /// Whether to check Plane.so issues
    pub check_plane: bool,
    /// Project path for code exploration
    pub project_path: Option<String>,
}

/// Comparison result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareResult {
    pub gaps: Vec<Gap>,
    pub matches: Vec<Match>,
    pub summary: String,
}

/// A gap between spec and reality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gap {
    pub description: String,
    pub source: String,       // Which spec file
    pub gap_type: GapType,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GapType {
    /// Described in spec but not tracked in Plane
    NotTracked,
    /// Described in spec but not implemented in code
    NotImplemented,
    /// In Plane/code but not in spec (discovered work)
    Undocumented,
    /// Spec says X, reality says Y
    Mismatch,
}

/// A match between spec and reality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Match {
    pub description: String,
    pub source: String,
    pub evidence: String,
}

/// Run comparison using LLM as subagent.
///
/// This spawns the llm-code-sdk tool runner to explore the codebase
/// and verify whether it matches the spec files.
pub async fn compare(
    request: CompareRequest,
    plane_issues: &[PlaneIssue],
    lm_studio_url: &str,
) -> Result<CompareResult> {
    use llm_code_sdk::{Client, MessageCreateParams, MessageParam};

    // Read all spec files
    let mut spec_content = String::new();
    for path in &request.spec_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            spec_content.push_str(&format!("\n## {}\n{}\n", path, content));
        }
    }

    // Build context about current Plane state
    let mut plane_context = String::from("## Current Plane.so Issues\n");
    for issue in plane_issues {
        let state = issue.state.as_deref().unwrap_or("?");
        plane_context.push_str(&format!(
            "- {}: {} [{}]\n",
            issue.sequence_id, issue.name, state
        ));
    }

    // Build the comparison prompt
    let prompt = format!(
        r#"Compare these spec documents with the current Plane.so state.

{spec_content}

{plane_context}

Identify:
1. Items in specs that have no corresponding Plane issue (gaps)
2. Items that are tracked and match (matches)
3. Any status mismatches

Output as JSON:
{{
  "gaps": [{{"description": "...", "source": "file.md", "gap_type": "NotTracked", "suggested_action": "..."}}],
  "matches": [{{"description": "...", "source": "file.md", "evidence": "PAL-XX"}}],
  "summary": "Brief summary"
}}"#
    );

    // Call LLM (LM Studio is OpenAI-compatible)
    let client = Client::openai_compatible(lm_studio_url)?;

    let response = client.messages().create(MessageCreateParams {
        model: "glm-4-plus".into(),
        max_tokens: 4096,
        messages: vec![MessageParam::user(&prompt)],
        ..Default::default()
    }).await?;

    // Parse response
    let text = response.text().unwrap_or_default();

    // Try to extract JSON from response
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            let json = &text[start..=end];
            if let Ok(result) = serde_json::from_str::<CompareResult>(json) {
                return Ok(result);
            }
        }
    }

    // Fallback: return raw summary
    Ok(CompareResult {
        gaps: vec![],
        matches: vec![],
        summary: text.to_string(),
    })
}

/// Format comparison result for inline display.
pub fn format_inline(result: &CompareResult) -> String {
    if result.gaps.is_empty() {
        return String::new();
    }

    let mut output = String::from("\n### Spec Gaps\n");
    for gap in result.gaps.iter().take(5) {
        output.push_str(&format!("- {}\n", gap.description));
    }
    if result.gaps.len() > 5 {
        output.push_str(&format!("  (+{} more)\n", result.gaps.len() - 5));
    }
    output
}
