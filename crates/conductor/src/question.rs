//! Question generation and caching for Conductor.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// A question in the interview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    /// Unique identifier.
    pub id: Uuid,

    /// Question text.
    pub text: String,

    /// Available options.
    pub options: Vec<QuestionOption>,

    /// Context that generated this question.
    pub context: String,

    /// Parent question ID if this is a follow-up.
    pub parent_id: Option<Uuid>,

    /// Tags for categorization.
    pub tags: Vec<String>,

    /// Priority (higher = more important).
    pub priority: u8,

    /// Whether this question is still relevant.
    pub relevant: bool,

    /// Cache key for regeneration detection.
    pub cache_key: String,
}

impl Question {
    /// Create a new question.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cache_key = format!("{:x}", md5::compute(&text));

        Self {
            id: Uuid::new_v4(),
            text,
            options: vec![],
            context: String::new(),
            parent_id: None,
            tags: vec![],
            priority: 5,
            relevant: true,
            cache_key,
        }
    }

    /// Add an option.
    pub fn with_option(mut self, option: QuestionOption) -> Self {
        self.options.push(option);
        self
    }

    /// Add multiple options.
    pub fn with_options(mut self, options: Vec<QuestionOption>) -> Self {
        self.options.extend(options);
        self
    }

    /// Set context.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = context.into();
        self
    }

    /// Set parent.
    pub fn with_parent(mut self, parent_id: Uuid) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Add a tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Mark as irrelevant.
    pub fn mark_irrelevant(&mut self) {
        self.relevant = false;
    }

    /// Check if question needs regeneration based on context change.
    pub fn needs_regeneration(&self, new_context: &str) -> bool {
        let new_key = format!("{:x}", md5::compute(new_context));
        new_key != self.cache_key
    }
}

/// An option for a question.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    /// Option identifier.
    pub id: Uuid,

    /// Option label (short).
    pub label: String,

    /// Option description (longer).
    pub description: Option<String>,

    /// Follow-up questions this option triggers.
    pub follow_up_ids: Vec<Uuid>,

    /// Whether this is a terminal option (ends this branch).
    pub terminal: bool,

    /// Value to record if selected.
    pub value: serde_json::Value,
}

impl QuestionOption {
    /// Create a new option.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            description: None,
            follow_up_ids: vec![],
            terminal: false,
            value: serde_json::Value::Null,
        }
    }

    /// Set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add a follow-up question.
    pub fn triggers(mut self, question_id: Uuid) -> Self {
        self.follow_up_ids.push(question_id);
        self
    }

    /// Mark as terminal.
    pub fn terminal(mut self) -> Self {
        self.terminal = true;
        self
    }

    /// Set value.
    pub fn with_value(mut self, value: serde_json::Value) -> Self {
        self.value = value;
        self
    }
}

/// An answer to a question.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    /// Question this answers.
    pub question_id: Uuid,

    /// Selected option ID.
    pub option_id: Uuid,

    /// Free-form text if "Other" was selected.
    pub custom_text: Option<String>,

    /// Timestamp.
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// Confidence (0.0 - 1.0) if user indicated.
    pub confidence: Option<f32>,
}

impl Answer {
    /// Create a new answer.
    pub fn new(question_id: Uuid, option_id: Uuid) -> Self {
        Self {
            question_id,
            option_id,
            custom_text: None,
            timestamp: chrono::Utc::now(),
            confidence: None,
        }
    }

    /// Set custom text.
    pub fn with_custom_text(mut self, text: impl Into<String>) -> Self {
        self.custom_text = Some(text.into());
        self
    }

    /// Set confidence.
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence.clamp(0.0, 1.0));
        self
    }
}

/// Cache for questions to avoid regeneration.
#[derive(Debug, Default)]
pub struct QuestionCache {
    questions: HashMap<String, Question>,
    context_hashes: HashMap<Uuid, String>,
}

impl QuestionCache {
    /// Create a new cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a cached question.
    pub fn get(&self, cache_key: &str) -> Option<&Question> {
        self.questions.get(cache_key)
    }

    /// Cache a question.
    pub fn insert(&mut self, question: Question) {
        self.context_hashes
            .insert(question.id, question.cache_key.clone());
        self.questions.insert(question.cache_key.clone(), question);
    }

    /// Check if a question needs regeneration.
    pub fn needs_regeneration(&self, question_id: Uuid, new_context: &str) -> bool {
        if let Some(old_hash) = self.context_hashes.get(&question_id) {
            let new_hash = format!("{:x}", md5::compute(new_context));
            return new_hash != *old_hash;
        }
        true // Not cached, needs generation
    }

    /// Prune irrelevant questions.
    pub fn prune(&mut self) {
        self.questions.retain(|_, q| q.relevant);
        let relevant_ids: std::collections::HashSet<_> =
            self.questions.values().map(|q| q.id).collect();
        self.context_hashes
            .retain(|id, _| relevant_ids.contains(id));
    }

    /// Clear the cache.
    pub fn clear(&mut self) {
        self.questions.clear();
        self.context_hashes.clear();
    }

    /// Get cache size.
    pub fn len(&self) -> usize {
        self.questions.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.questions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_question_creation() {
        let question = Question::new("What framework?")
            .with_option(QuestionOption::new("React"))
            .with_option(QuestionOption::new("Vue"))
            .with_priority(8);

        assert_eq!(question.options.len(), 2);
        assert_eq!(question.priority, 8);
    }

    #[test]
    fn test_question_cache() {
        let mut cache = QuestionCache::new();

        let question = Question::new("Test question");
        cache.insert(question.clone());

        assert!(cache.get(&question.cache_key).is_some());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_needs_regeneration() {
        let question = Question::new("Test").with_context("Context A");
        assert!(!question.needs_regeneration("Test")); // Same text
        assert!(question.needs_regeneration("Different context")); // Different
    }
}
