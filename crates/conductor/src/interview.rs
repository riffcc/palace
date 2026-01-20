//! Interview tree for Conductor.

use crate::error::{ConductorError, ConductorResult};
use crate::question::{Answer, Question, QuestionCache, QuestionOption};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// State of an interview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterviewState {
    /// Not started.
    NotStarted,
    /// In progress.
    InProgress,
    /// Completed.
    Completed,
    /// Paused.
    Paused,
    /// Cancelled.
    Cancelled,
}

impl Default for InterviewState {
    fn default() -> Self {
        InterviewState::NotStarted
    }
}

/// An interview session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interview {
    /// Interview ID.
    pub id: Uuid,

    /// Topic being explored.
    pub topic: String,

    /// Current state.
    pub state: InterviewState,

    /// All answers collected.
    pub answers: Vec<Answer>,

    /// Final decisions extracted.
    pub decisions: HashMap<String, serde_json::Value>,

    /// Started timestamp.
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,

    /// Completed timestamp.
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Interview {
    /// Create a new interview.
    pub fn new(topic: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            topic: topic.into(),
            state: InterviewState::NotStarted,
            answers: vec![],
            decisions: HashMap::new(),
            started_at: None,
            completed_at: None,
        }
    }

    /// Start the interview.
    pub fn start(&mut self) {
        self.state = InterviewState::InProgress;
        self.started_at = Some(chrono::Utc::now());
    }

    /// Complete the interview.
    pub fn complete(&mut self) {
        self.state = InterviewState::Completed;
        self.completed_at = Some(chrono::Utc::now());
    }

    /// Pause the interview.
    pub fn pause(&mut self) {
        self.state = InterviewState::Paused;
    }

    /// Resume the interview.
    pub fn resume(&mut self) {
        if self.state == InterviewState::Paused {
            self.state = InterviewState::InProgress;
        }
    }

    /// Cancel the interview.
    pub fn cancel(&mut self) {
        self.state = InterviewState::Cancelled;
    }

    /// Add an answer.
    pub fn add_answer(&mut self, answer: Answer) {
        self.answers.push(answer);
    }

    /// Record a decision.
    pub fn record_decision(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.decisions.insert(key.into(), value);
    }

    /// Get progress (0.0 - 1.0).
    pub fn progress(&self) -> f32 {
        // This would be calculated based on the interview tree depth
        // For now, estimate based on answers
        let min_answers = 3;
        (self.answers.len() as f32 / min_answers as f32).min(1.0)
    }
}

/// The interview tree that manages branching questions.
#[derive(Debug)]
pub struct InterviewTree {
    /// Current interview.
    pub interview: Option<Interview>,

    /// All questions in the tree.
    questions: HashMap<Uuid, Question>,

    /// Root question IDs.
    root_questions: Vec<Uuid>,

    /// Current question ID.
    current_question_id: Option<Uuid>,

    /// Question history (for back navigation).
    history: Vec<Uuid>,

    /// Question cache.
    cache: QuestionCache,
}

impl InterviewTree {
    /// Create a new interview tree.
    pub fn new() -> Self {
        Self {
            interview: None,
            questions: HashMap::new(),
            root_questions: vec![],
            current_question_id: None,
            history: vec![],
            cache: QuestionCache::new(),
        }
    }

    /// Start a new interview.
    pub fn start(&mut self, topic: String) {
        let mut interview = Interview::new(&topic);
        interview.start();
        self.interview = Some(interview);

        // Generate initial questions for the topic
        self.generate_initial_questions(&topic);
    }

    /// Generate initial questions for a topic.
    fn generate_initial_questions(&mut self, topic: &str) {
        // In a real implementation, this would use the LLM to generate
        // contextually relevant questions based on the topic

        // Placeholder initial question
        let question = Question::new(format!("What aspect of '{}' would you like to explore?", topic))
            .with_option(QuestionOption::new("Architecture").with_description("System design and structure"))
            .with_option(QuestionOption::new("Implementation").with_description("Code and technical details"))
            .with_option(QuestionOption::new("Testing").with_description("Test strategy and coverage"))
            .with_option(QuestionOption::new("Deployment").with_description("CI/CD and infrastructure"));

        let question_id = question.id;
        self.questions.insert(question_id, question);
        self.root_questions.push(question_id);
        self.current_question_id = Some(question_id);
    }

    /// Get the current question.
    pub fn current_question(&self) -> Option<Question> {
        self.current_question_id
            .and_then(|id| self.questions.get(&id))
            .cloned()
    }

    /// Submit an answer.
    pub fn submit_answer(&mut self, answer: Answer) -> ConductorResult<()> {
        // Extract needed data first to avoid borrow issues
        let (question_tags, question_id, option_terminal, option_value, option_follow_up_ids, option_label) = {
            let question = self
                .questions
                .get(&answer.question_id)
                .ok_or_else(|| ConductorError::Interview("Question not found".into()))?;

            let option = question
                .options
                .iter()
                .find(|o| o.id == answer.option_id)
                .ok_or_else(|| ConductorError::InvalidAnswer("Option not found".into()))?;

            (
                question.tags.clone(),
                question.id,
                option.terminal,
                option.value.clone(),
                option.follow_up_ids.clone(),
                option.label.clone(),
            )
        };

        let Some(ref mut interview) = self.interview else {
            return Err(ConductorError::Interview("No active interview".into()));
        };

        // Record the answer
        interview.add_answer(answer.clone());

        // Record as decision if terminal
        if option_terminal {
            interview.record_decision(
                question_tags.first().cloned().unwrap_or_else(|| question_id.to_string()),
                option_value,
            );
        }

        // Save current to history
        if let Some(current_id) = self.current_question_id {
            self.history.push(current_id);
        }

        // Navigate to follow-up or next question
        if let Some(follow_up_id) = option_follow_up_ids.first() {
            self.current_question_id = Some(*follow_up_id);
        } else if option_terminal {
            // Move to next root question or complete
            self.advance_to_next_question();
        } else {
            // Generate follow-up questions dynamically
            self.generate_follow_up_for_label(&option_label);
        }

        Ok(())
    }

    /// Advance to the next question.
    fn advance_to_next_question(&mut self) {
        // Find next unanswered root question
        let answered_ids: std::collections::HashSet<_> = self
            .interview
            .as_ref()
            .map(|i| i.answers.iter().map(|a| a.question_id).collect())
            .unwrap_or_default();

        for root_id in &self.root_questions {
            if !answered_ids.contains(root_id) {
                self.current_question_id = Some(*root_id);
                return;
            }
        }

        // All done
        self.current_question_id = None;
        if let Some(ref mut interview) = self.interview {
            interview.complete();
        }
    }

    /// Generate follow-up questions based on an answer label.
    fn generate_follow_up_for_label(&mut self, label: &str) {
        // In a real implementation, this would use the LLM to generate
        // contextual follow-up questions based on the answer

        // Placeholder - create a simple follow-up
        let follow_up = Question::new(format!("Can you tell me more about '{}'?", label))
            .with_option(QuestionOption::new("Yes, here's more detail").terminal())
            .with_option(QuestionOption::new("That's all for now").terminal());

        let follow_up_id = follow_up.id;
        self.questions.insert(follow_up_id, follow_up);
        self.current_question_id = Some(follow_up_id);
    }

    /// Go back to the previous question.
    pub fn go_back(&mut self) -> Option<Question> {
        if let Some(prev_id) = self.history.pop() {
            self.current_question_id = Some(prev_id);
            return self.current_question();
        }
        None
    }

    /// Get all collected decisions.
    pub fn decisions(&self) -> Option<&HashMap<String, serde_json::Value>> {
        self.interview.as_ref().map(|i| &i.decisions)
    }

    /// Check if interview is complete.
    pub fn is_complete(&self) -> bool {
        self.interview
            .as_ref()
            .map(|i| i.state == InterviewState::Completed)
            .unwrap_or(false)
    }

    /// Get interview progress.
    pub fn progress(&self) -> f32 {
        self.interview.as_ref().map(|i| i.progress()).unwrap_or(0.0)
    }

    /// Regenerate questions based on new context.
    pub fn regenerate_for_context(&mut self, new_context: &str) {
        // Mark questions that need regeneration
        for question in self.questions.values_mut() {
            if question.needs_regeneration(new_context) {
                question.mark_irrelevant();
            }
        }

        // Prune cache
        self.cache.prune();

        // In a real implementation, would regenerate needed questions
    }
}

impl Default for InterviewTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interview_lifecycle() {
        let mut interview = Interview::new("Test topic");
        assert_eq!(interview.state, InterviewState::NotStarted);

        interview.start();
        assert_eq!(interview.state, InterviewState::InProgress);

        interview.pause();
        assert_eq!(interview.state, InterviewState::Paused);

        interview.resume();
        assert_eq!(interview.state, InterviewState::InProgress);

        interview.complete();
        assert_eq!(interview.state, InterviewState::Completed);
    }

    #[test]
    fn test_interview_tree() {
        let mut tree = InterviewTree::new();
        tree.start("Test project".into());

        assert!(tree.interview.is_some());
        assert!(tree.current_question().is_some());
    }

    #[test]
    fn test_submit_answer() {
        let mut tree = InterviewTree::new();
        tree.start("Test".into());

        let question = tree.current_question().unwrap();
        let option = &question.options[0];

        let answer = Answer::new(question.id, option.id);
        tree.submit_answer(answer).unwrap();

        // Should have advanced to next question
        assert!(!tree.history.is_empty());
    }
}
