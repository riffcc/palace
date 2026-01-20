//! Recursive Survey - Multi-question polls with dependency tracking.
//!
//! Posts all questions upfront as separate Zulip messages (polls),
//! detects answers automatically via Zulip API polling, and regenerates
//! dependent questions based on the answers received.
//!
//! # Design
//!
//! - Each question is a separate poll message
//! - Questions can depend on other questions' answers
//! - When an answer changes, dependent questions are regenerated
//! - Supports conditional questions (show only if X was answered Y)

use crate::zulip_tool::ZulipTool;
use crate::{DirectorError, DirectorResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A survey question with optional dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurveyQuestion {
    /// Unique ID for this question.
    pub id: String,
    /// Question text (shown in poll).
    pub question: String,
    /// Available options.
    pub options: Vec<String>,
    /// Optional: only show if dependency question has specific answer.
    pub depends_on: Option<QuestionDependency>,
    /// Template for regenerating question text based on other answers.
    /// Use {answer_id} to interpolate answers from other questions.
    pub template: Option<String>,
}

/// Dependency specification for conditional questions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionDependency {
    /// ID of question this depends on.
    pub question_id: String,
    /// Only show if answer matches this value (or any in list).
    pub requires_answer: Vec<String>,
}

/// A complete survey definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurveyDefinition {
    /// Survey title (shown in header).
    pub title: String,
    /// Optional description.
    pub description: Option<String>,
    /// The questions in order.
    pub questions: Vec<SurveyQuestion>,
    /// Who to tag on completion.
    pub notify_on_complete: Vec<String>,
}

/// Tracks a live survey's state.
pub struct RecursiveSurvey {
    tool: ZulipTool,
    /// Stream to post to.
    stream: String,
    /// Topic for this survey.
    topic: String,
    /// Survey definition.
    definition: SurveyDefinition,
    /// Message IDs for each question (by question ID).
    message_ids: HashMap<String, u64>,
    /// Current answers (by question ID).
    answers: HashMap<String, String>,
    /// Header message ID.
    header_message_id: Option<u64>,
    /// Footer/status message ID.
    status_message_id: Option<u64>,
}

impl RecursiveSurvey {
    /// Create a new survey.
    pub fn new(
        tool: ZulipTool,
        stream: &str,
        topic: &str,
        definition: SurveyDefinition,
    ) -> Self {
        Self {
            tool,
            stream: stream.to_string(),
            topic: topic.to_string(),
            definition,
            message_ids: HashMap::new(),
            answers: HashMap::new(),
            header_message_id: None,
            status_message_id: None,
        }
    }

    /// Post the entire survey upfront.
    /// Returns the message IDs for all questions.
    pub async fn post(&mut self) -> DirectorResult<()> {
        // Post header
        let header = self.render_header();
        let header_id = self.tool.send(&self.stream, &self.topic, &header).await?;
        self.header_message_id = Some(header_id);

        // Post each question
        for question in &self.definition.questions {
            if self.should_show_question(question) {
                let rendered = self.render_question(question);
                let options: Vec<&str> = question.options.iter().map(|s| s.as_str()).collect();
                let msg_id = self.tool.send_poll(&self.stream, &self.topic, &rendered, &options).await?;
                self.message_ids.insert(question.id.clone(), msg_id);
            }
        }

        // Post status footer
        let status = self.render_status();
        let status_id = self.tool.send(&self.stream, &self.topic, &status).await?;
        self.status_message_id = Some(status_id);

        Ok(())
    }

    /// Check for new answers by polling the Zulip API.
    /// Returns true if any answers changed.
    pub async fn poll_answers(&mut self) -> DirectorResult<bool> {
        let mut changed = false;

        // Get messages from the topic to check poll states
        let messages = self.tool.get_messages(&self.stream, Some(&self.topic), 50).await?;

        for (question_id, &msg_id) in &self.message_ids.clone() {
            // Find the message and check for poll votes
            if let Some(msg) = messages.iter().find(|m| m.id == msg_id) {
                // Parse poll results from message content
                // Zulip poll messages have a specific format we can detect
                if let Some(answer) = self.extract_poll_answer(&msg.content) {
                    if self.answers.get(question_id) != Some(&answer) {
                        self.answers.insert(question_id.clone(), answer);
                        changed = true;
                    }
                }
            }
        }

        Ok(changed)
    }

    /// Regenerate questions that depend on changed answers.
    pub async fn regenerate_dependent(&mut self) -> DirectorResult<()> {
        for question in &self.definition.questions.clone() {
            // Check if this question depends on an answered question
            if let Some(ref dep) = question.depends_on {
                if let Some(answer) = self.answers.get(&dep.question_id) {
                    let should_show = dep.requires_answer.contains(answer);
                    let is_shown = self.message_ids.contains_key(&question.id);

                    if should_show && !is_shown {
                        // Need to add this question
                        let rendered = self.render_question(question);
                        let options: Vec<&str> = question.options.iter().map(|s| s.as_str()).collect();
                        let msg_id = self.tool.send_poll(&self.stream, &self.topic, &rendered, &options).await?;
                        self.message_ids.insert(question.id.clone(), msg_id);
                    } else if !should_show && is_shown {
                        // Need to remove this question
                        if let Some(msg_id) = self.message_ids.remove(&question.id) {
                            let _ = self.tool.delete_message(msg_id).await;
                        }
                    }
                }
            }

            // Check if question text needs regeneration (template with interpolation)
            if question.template.is_some() && self.message_ids.contains_key(&question.id) {
                // Delete and recreate with new text
                if let Some(old_id) = self.message_ids.remove(&question.id) {
                    let _ = self.tool.delete_message(old_id).await;
                }
                let rendered = self.render_question(question);
                let options: Vec<&str> = question.options.iter().map(|s| s.as_str()).collect();
                let msg_id = self.tool.send_poll(&self.stream, &self.topic, &rendered, &options).await?;
                self.message_ids.insert(question.id.clone(), msg_id);
            }
        }

        // Update status
        self.update_status().await?;

        Ok(())
    }

    /// Update the status message.
    pub async fn update_status(&mut self) -> DirectorResult<()> {
        let status = self.render_status();
        if let Some(msg_id) = self.status_message_id {
            self.tool.update_message(msg_id, &status).await?;
        }
        Ok(())
    }

    /// Check if survey is complete (all required questions answered).
    pub fn is_complete(&self) -> bool {
        for question in &self.definition.questions {
            if self.should_show_question(question) && !self.answers.contains_key(&question.id) {
                return false;
            }
        }
        true
    }

    /// Get all answers.
    pub fn answers(&self) -> &HashMap<String, String> {
        &self.answers
    }

    /// Run the survey loop until complete or timeout.
    pub async fn run(&mut self, poll_interval_secs: u64, timeout_secs: u64) -> DirectorResult<HashMap<String, String>> {
        // Post the survey
        self.post().await?;

        let start = std::time::Instant::now();
        let poll_interval = std::time::Duration::from_secs(poll_interval_secs);
        let timeout = std::time::Duration::from_secs(timeout_secs);

        while !self.is_complete() {
            if start.elapsed() > timeout {
                return Err(DirectorError::Other("Survey timed out".to_string()));
            }

            tokio::time::sleep(poll_interval).await;

            if self.poll_answers().await? {
                self.regenerate_dependent().await?;
            }
        }

        // Post completion message
        self.complete().await?;

        Ok(self.answers.clone())
    }

    /// Mark survey as complete and notify.
    async fn complete(&mut self) -> DirectorResult<()> {
        // Update status to complete
        let mut complete_msg = String::from("✅ **Survey Complete**\n\n");
        for question in &self.definition.questions {
            if let Some(answer) = self.answers.get(&question.id) {
                complete_msg.push_str(&format!("- **{}**: {}\n", question.question, answer));
            }
        }

        if !self.definition.notify_on_complete.is_empty() {
            complete_msg.push_str("\n");
            for user in &self.definition.notify_on_complete {
                complete_msg.push_str(&format!("@**{}** ", user));
            }
        }

        if let Some(msg_id) = self.status_message_id {
            self.tool.update_message(msg_id, &complete_msg).await?;
        }

        Ok(())
    }

    // --- Private helpers ---

    fn should_show_question(&self, question: &SurveyQuestion) -> bool {
        if let Some(ref dep) = question.depends_on {
            if let Some(answer) = self.answers.get(&dep.question_id) {
                return dep.requires_answer.contains(answer);
            }
            // Dependency not answered yet - don't show
            return false;
        }
        // No dependency - always show
        true
    }

    fn render_question(&self, question: &SurveyQuestion) -> String {
        if let Some(ref template) = question.template {
            // Interpolate answers into template
            let mut text = template.clone();
            for (id, answer) in &self.answers {
                text = text.replace(&format!("{{{}}}", id), answer);
            }
            text
        } else {
            question.question.clone()
        }
    }

    fn render_header(&self) -> String {
        let mut header = format!("## 📊 {}\n\n", self.definition.title);
        if let Some(ref desc) = self.definition.description {
            header.push_str(desc);
            header.push_str("\n\n");
        }
        header.push_str("*Vote on each question below. Some questions may appear based on your answers.*\n");
        header
    }

    fn render_status(&self) -> String {
        let total = self.definition.questions.iter()
            .filter(|q| self.should_show_question(q))
            .count();
        let answered = self.answers.len();
        let progress = if total > 0 { (answered * 100) / total } else { 0 };
        let bar = progress_bar(progress as u32);

        format!(
            "📈 **Progress:** {} {}/{} ({}%)\n\n\
            *Waiting for answers...*",
            bar, answered, total, progress
        )
    }

    fn extract_poll_answer(&self, content: &str) -> Option<String> {
        // Zulip poll results appear in the message when votes are cast.
        // This is a simplified extraction - real implementation would need
        // to use the messages API with include_submessages for poll data.

        // For now, we look for common patterns in poll responses
        // The actual poll data is in submessages, but we can detect
        // voted options from the rendered HTML/text

        // Look for "✓" or voting indicators
        for line in content.lines() {
            if line.contains("✓") || line.contains("voted") {
                // Extract the option text
                if let Some(option) = line.split(':').last() {
                    let option = option.trim();
                    if !option.is_empty() {
                        return Some(option.to_string());
                    }
                }
            }
        }

        None
    }
}

/// Generate a text progress bar.
fn progress_bar(percent: u32) -> String {
    let filled = (percent / 10) as usize;
    let empty = 10 - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

/// Builder for creating surveys easily.
pub struct SurveyBuilder {
    title: String,
    description: Option<String>,
    questions: Vec<SurveyQuestion>,
    notify: Vec<String>,
}

impl SurveyBuilder {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            description: None,
            questions: Vec::new(),
            notify: Vec::new(),
        }
    }

    pub fn description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    pub fn question(mut self, id: &str, question: &str, options: &[&str]) -> Self {
        self.questions.push(SurveyQuestion {
            id: id.to_string(),
            question: question.to_string(),
            options: options.iter().map(|s| s.to_string()).collect(),
            depends_on: None,
            template: None,
        });
        self
    }

    pub fn conditional_question(
        mut self,
        id: &str,
        question: &str,
        options: &[&str],
        depends_on: &str,
        requires: &[&str],
    ) -> Self {
        self.questions.push(SurveyQuestion {
            id: id.to_string(),
            question: question.to_string(),
            options: options.iter().map(|s| s.to_string()).collect(),
            depends_on: Some(QuestionDependency {
                question_id: depends_on.to_string(),
                requires_answer: requires.iter().map(|s| s.to_string()).collect(),
            }),
            template: None,
        });
        self
    }

    pub fn templated_question(
        mut self,
        id: &str,
        template: &str,
        options: &[&str],
    ) -> Self {
        self.questions.push(SurveyQuestion {
            id: id.to_string(),
            question: template.to_string(), // Fallback
            options: options.iter().map(|s| s.to_string()).collect(),
            depends_on: None,
            template: Some(template.to_string()),
        });
        self
    }

    pub fn notify_on_complete(mut self, users: &[&str]) -> Self {
        self.notify = users.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn build(self) -> SurveyDefinition {
        SurveyDefinition {
            title: self.title,
            description: self.description,
            questions: self.questions,
            notify_on_complete: self.notify,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_survey_builder() {
        let survey = SurveyBuilder::new("Feature Priority")
            .description("Help us prioritize the next features")
            .question("q1", "Most important area?", &["Performance", "Features", "UX"])
            .conditional_question(
                "q1_perf",
                "Which performance aspect?",
                &["Latency", "Throughput", "Memory"],
                "q1",
                &["Performance"],
            )
            .notify_on_complete(&["wings"])
            .build();

        assert_eq!(survey.title, "Feature Priority");
        assert_eq!(survey.questions.len(), 2);
        assert!(survey.questions[1].depends_on.is_some());
    }

    #[test]
    fn test_progress_bar() {
        assert_eq!(progress_bar(0), "[░░░░░░░░░░]");
        assert_eq!(progress_bar(50), "[█████░░░░░]");
        assert_eq!(progress_bar(100), "[██████████]");
    }
}
