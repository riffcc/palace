//! Reaction-based feedback system.
//!
//! Uses emoji reactions as a real-time steering mechanism:
//! - ❤️ (heart) = great job, happy with that idea
//! - 👍 (thumbs_up) = soft approval, proceed
//! - 👎 (thumbs_down) = soft disapproval, reconsider
//! - 🛑 (stop) = halt immediately, await feedback
//! - ⏸️ (pause) = pause and wait for clarification
//! - ✅ (check) = confirmed, execute
//! - ❌ (x) = rejected, don't do this
//! - 🔄 (refresh) = retry/redo
//! - 💡 (bulb) = interesting idea, explore further
//! - ⚠️ (warning) = proceed with caution
//!
//! After reacting, users can optionally follow up with a message
//! explaining their reaction.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Feedback from a reaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Feedback {
    /// Strong positive - great idea, love it.
    StrongPositive,
    /// Soft positive - acceptable, proceed.
    SoftPositive,
    /// Neutral - noted, no strong opinion.
    Neutral,
    /// Soft negative - questionable, reconsider.
    SoftNegative,
    /// Strong negative - bad idea, don't do this.
    StrongNegative,
    /// Halt - stop everything and wait.
    Halt,
    /// Explore - interesting, dig deeper.
    Explore,
    /// Caution - be careful.
    Caution,
    /// Retry - do it again.
    Retry,
    /// Confirm - explicit approval to execute.
    Confirm,
}

impl Feedback {
    /// Parse from emoji name.
    pub fn from_emoji(emoji: &str) -> Option<Self> {
        match emoji {
            // Strong positive
            "heart" | "heart_eyes" | "star" | "tada" | "fire" | "+1" => {
                Some(Feedback::StrongPositive)
            }
            // Soft positive
            "thumbsup" | "thumbs_up" | "ok" | "ok_hand" | "slightly_smiling_face" => {
                Some(Feedback::SoftPositive)
            }
            // Soft negative
            "thumbsdown" | "thumbs_down" | "-1" | "thinking" | "confused" => {
                Some(Feedback::SoftNegative)
            }
            // Strong negative
            "x" | "no_entry" | "no_entry_sign" | "rage" | "angry" => {
                Some(Feedback::StrongNegative)
            }
            // Halt signals
            "octagonal_sign" | "stop_sign" | "raised_hand" | "hand" | "stop" => {
                Some(Feedback::Halt)
            }
            // Explore
            "bulb" | "mag" | "eyes" | "point_right" => {
                Some(Feedback::Explore)
            }
            // Caution
            "warning" | "construction" | "exclamation" => {
                Some(Feedback::Caution)
            }
            // Retry
            "repeat" | "arrows_counterclockwise" | "recycle" | "refresh" => {
                Some(Feedback::Retry)
            }
            // Confirm
            "white_check_mark" | "heavy_check_mark" | "check" | "ballot_box_with_check" => {
                Some(Feedback::Confirm)
            }
            // Neutral/other
            _ => None,
        }
    }

    /// Check if this feedback requires waiting for more input.
    pub fn requires_wait(&self) -> bool {
        matches!(self, Feedback::Halt | Feedback::StrongNegative)
    }

    /// Check if this is positive feedback.
    pub fn is_positive(&self) -> bool {
        matches!(self, Feedback::StrongPositive | Feedback::SoftPositive | Feedback::Confirm)
    }

    /// Check if this is negative feedback.
    pub fn is_negative(&self) -> bool {
        matches!(self, Feedback::SoftNegative | Feedback::StrongNegative)
    }

    /// Get a weight for this feedback (-1.0 to 1.0).
    pub fn weight(&self) -> f64 {
        match self {
            Feedback::StrongPositive => 1.0,
            Feedback::SoftPositive => 0.5,
            Feedback::Neutral => 0.0,
            Feedback::SoftNegative => -0.5,
            Feedback::StrongNegative => -1.0,
            Feedback::Halt => -1.0,
            Feedback::Explore => 0.3,
            Feedback::Caution => -0.2,
            Feedback::Retry => 0.0,
            Feedback::Confirm => 1.0,
        }
    }

    /// Get suggested action text.
    pub fn action_hint(&self) -> &'static str {
        match self {
            Feedback::StrongPositive => "Great! Continuing with confidence.",
            Feedback::SoftPositive => "Proceeding as planned.",
            Feedback::Neutral => "Noted. Continuing.",
            Feedback::SoftNegative => "Reconsidering approach...",
            Feedback::StrongNegative => "Stopping this approach.",
            Feedback::Halt => "Halting. Waiting for clarification.",
            Feedback::Explore => "Interesting! Exploring further.",
            Feedback::Caution => "Proceeding carefully.",
            Feedback::Retry => "Retrying...",
            Feedback::Confirm => "Confirmed. Executing.",
        }
    }
}

/// A reaction event with context.
#[derive(Debug, Clone)]
pub struct ReactionEvent {
    /// Message ID that was reacted to.
    pub message_id: u64,
    /// User ID who reacted.
    pub user_id: u64,
    /// User's display name.
    pub user_name: String,
    /// The emoji name.
    pub emoji: String,
    /// Parsed feedback.
    pub feedback: Option<Feedback>,
    /// Timestamp.
    pub timestamp: Instant,
    /// Follow-up message if any.
    pub follow_up: Option<String>,
}

impl ReactionEvent {
    /// Create from a Zulip reaction.
    pub fn new(message_id: u64, user_id: u64, user_name: impl Into<String>, emoji: impl Into<String>) -> Self {
        let emoji = emoji.into();
        let feedback = Feedback::from_emoji(&emoji);
        Self {
            message_id,
            user_id,
            user_name: user_name.into(),
            emoji,
            feedback,
            timestamp: Instant::now(),
            follow_up: None,
        }
    }

    /// Add follow-up message.
    pub fn with_follow_up(mut self, message: impl Into<String>) -> Self {
        self.follow_up = Some(message.into());
        self
    }

    /// Check if this reaction requires halting.
    pub fn requires_halt(&self) -> bool {
        self.feedback.map(|f| f.requires_wait()).unwrap_or(false)
    }
}

/// Tracks reactions on messages for feedback aggregation.
#[derive(Debug, Default)]
pub struct ReactionTracker {
    /// Reactions by message ID.
    reactions: HashMap<u64, Vec<ReactionEvent>>,
    /// Follow-up window duration.
    follow_up_window: Duration,
}

impl ReactionTracker {
    /// Create a new tracker.
    pub fn new() -> Self {
        Self {
            reactions: HashMap::new(),
            follow_up_window: Duration::from_secs(30),
        }
    }

    /// Set follow-up window duration.
    pub fn with_follow_up_window(mut self, duration: Duration) -> Self {
        self.follow_up_window = duration;
        self
    }

    /// Record a reaction.
    pub fn record(&mut self, event: ReactionEvent) {
        self.reactions
            .entry(event.message_id)
            .or_default()
            .push(event);
    }

    /// Try to add follow-up message to recent reactions from a user.
    pub fn add_follow_up(&mut self, user_id: u64, message: &str) -> bool {
        let now = Instant::now();

        // Find the most recent reaction from this user within the window
        for reactions in self.reactions.values_mut() {
            for reaction in reactions.iter_mut().rev() {
                if reaction.user_id == user_id
                    && reaction.follow_up.is_none()
                    && now.duration_since(reaction.timestamp) < self.follow_up_window
                {
                    reaction.follow_up = Some(message.to_string());
                    return true;
                }
            }
        }
        false
    }

    /// Get all reactions for a message.
    pub fn get(&self, message_id: u64) -> &[ReactionEvent] {
        self.reactions.get(&message_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get aggregated feedback score for a message.
    pub fn aggregate_score(&self, message_id: u64) -> f64 {
        let reactions = self.get(message_id);
        if reactions.is_empty() {
            return 0.0;
        }

        let total: f64 = reactions.iter()
            .filter_map(|r| r.feedback)
            .map(|f| f.weight())
            .sum();

        total / reactions.len() as f64
    }

    /// Check if any reaction on a message requires halt.
    pub fn requires_halt(&self, message_id: u64) -> bool {
        self.get(message_id).iter().any(|r| r.requires_halt())
    }

    /// Get halt reasons if any.
    pub fn halt_reasons(&self, message_id: u64) -> Vec<String> {
        self.get(message_id)
            .iter()
            .filter(|r| r.requires_halt())
            .map(|r| {
                match &r.follow_up {
                    Some(msg) => format!("{}: {}", r.user_name, msg),
                    None => format!("{} signaled to halt ({})", r.user_name, r.emoji),
                }
            })
            .collect()
    }

    /// Clean up old reactions.
    pub fn cleanup(&mut self, max_age: Duration) {
        let now = Instant::now();
        for reactions in self.reactions.values_mut() {
            reactions.retain(|r| now.duration_since(r.timestamp) < max_age);
        }
        self.reactions.retain(|_, v| !v.is_empty());
    }
}

/// Survey question for recursive feedback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurveyQuestion {
    /// Question text.
    pub question: String,
    /// Options (emoji -> description).
    pub options: Vec<(String, String)>,
    /// Context/explanation.
    pub context: Option<String>,
}

impl SurveyQuestion {
    /// Create a yes/no question.
    pub fn yes_no(question: impl Into<String>) -> Self {
        Self {
            question: question.into(),
            options: vec![
                ("thumbsup".to_string(), "Yes".to_string()),
                ("thumbsdown".to_string(), "No".to_string()),
            ],
            context: None,
        }
    }

    /// Create a choice question.
    pub fn choice(question: impl Into<String>, options: Vec<(&str, &str)>) -> Self {
        Self {
            question: question.into(),
            options: options.into_iter()
                .map(|(e, d)| (e.to_string(), d.to_string()))
                .collect(),
            context: None,
        }
    }

    /// Create a scale question (1-5).
    pub fn scale(question: impl Into<String>) -> Self {
        Self {
            question: question.into(),
            options: vec![
                ("one".to_string(), "1 - Strongly disagree".to_string()),
                ("two".to_string(), "2 - Disagree".to_string()),
                ("three".to_string(), "3 - Neutral".to_string()),
                ("four".to_string(), "4 - Agree".to_string()),
                ("five".to_string(), "5 - Strongly agree".to_string()),
            ],
            context: None,
        }
    }

    /// Add context.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Format as Zulip message.
    pub fn to_message(&self) -> String {
        let mut msg = format!("**Survey**: {}\n\n", self.question);

        if let Some(ctx) = &self.context {
            msg.push_str(&format!("_{}_\n\n", ctx));
        }

        msg.push_str("React with:\n");
        for (emoji, desc) in &self.options {
            msg.push_str(&format!("- :{emoji}: = {desc}\n"));
        }

        msg
    }
}
