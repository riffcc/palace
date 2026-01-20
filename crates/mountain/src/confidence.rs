//! Confidence/Assurance level system for Mountain.
//!
//! The confidence slider controls the decision-maker vs feedback paradigm:
//!
//! **High Confidence (low assurance):**
//! - Small/fast models MAKE DECISIONS (immediate execution)
//! - Larger models provide REALTIME FEEDBACK (advisory, non-blocking)
//!
//! **Low Confidence (high assurance):**
//! - Large/smart models MAKE DECISIONS (delayed execution)
//! - Small models pre-process and inform the decision-maker
//!
//! **Overclock modes (OC-1, OC-2, OC-3):**
//! - Premium cloud models (Claude Opus 4.5, GPT-5.2, GPT-5.2-Pro) added for feedback
//! - OC-3 requires double d-pad press with haptic confirmation

use serde::{Deserialize, Serialize};

/// Assurance level - determines which model tier makes final decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AssuranceLevel {
    /// FAST - nvidia 8b orchestrator decides.
    /// Fastest response, all other models provide feedback.
    Fast,

    /// FLASH - glm-4.7-flash decides.
    /// Fast local model, larger models provide feedback.
    Flash,

    /// MINI - ministral-3-14b-reasoning decides.
    /// Local reasoning model, FULL provides feedback.
    Mini,

    /// FULL - glm-4.7 decides.
    /// Smartest local-tier model, no feedback (it's the top).
    Full,

    /// FULL + LOW OC - glm-4.7 decides.
    /// Adds realtime feedback from GPT-5.2 (cheaper).
    FullLowOC,

    /// FULL + MED OC - glm-4.7 decides.
    /// Adds Opus to the feedback council.
    FullMedOC,

    /// FULL + HIGH OC - glm-4.7 decides.
    /// Selective questions fed to GPT-5.2-Pro.
    /// Requires double d-pad press with haptic confirmation.
    FullHighOC,
}

impl AssuranceLevel {
    /// All levels in order (fastest to slowest/most assured).
    pub const ALL: &'static [AssuranceLevel] = &[
        AssuranceLevel::Fast,
        AssuranceLevel::Flash,
        AssuranceLevel::Mini,
        AssuranceLevel::Full,
        AssuranceLevel::FullLowOC,
        AssuranceLevel::FullMedOC,
        AssuranceLevel::FullHighOC,
    ];

    /// Get display name.
    pub fn name(&self) -> &'static str {
        match self {
            AssuranceLevel::Fast => "Fast",
            AssuranceLevel::Flash => "Flash",
            AssuranceLevel::Mini => "Mini",
            AssuranceLevel::Full => "Full",
            AssuranceLevel::FullLowOC => "Full (Low OC)",
            AssuranceLevel::FullMedOC => "Full (Med OC)",
            AssuranceLevel::FullHighOC => "Full (High OC)",
        }
    }

    /// Get short name for UI.
    pub fn short_name(&self) -> &'static str {
        match self {
            AssuranceLevel::Fast => "FAST",
            AssuranceLevel::Flash => "FLASH",
            AssuranceLevel::Mini => "MINI",
            AssuranceLevel::Full => "FULL",
            AssuranceLevel::FullLowOC => "LOW OC",
            AssuranceLevel::FullMedOC => "MED OC",
            AssuranceLevel::FullHighOC => "HIGH OC",
        }
    }

    /// Get description.
    pub fn description(&self) -> &'static str {
        match self {
            AssuranceLevel::Fast => {
                "nvidia 8b decides. All models give feedback."
            }
            AssuranceLevel::Flash => {
                "glm-4.7-flash decides. Larger models give feedback."
            }
            AssuranceLevel::Mini => {
                "ministral-3-14b decides. FULL gives feedback."
            }
            AssuranceLevel::Full => {
                "glm-4.7 decides. No feedback (it's the smartest)."
            }
            AssuranceLevel::FullLowOC => {
                "glm-4.7 decides + GPT-5.2 feedback."
            }
            AssuranceLevel::FullMedOC => {
                "glm-4.7 decides + GPT-5.2 + Opus feedback."
            }
            AssuranceLevel::FullHighOC => {
                "glm-4.7 decides + GPT-5.2-Pro selective. Double-tap."
            }
        }
    }

    /// Get the primary decision-making model for this level.
    pub fn decision_model(&self) -> &'static str {
        match self {
            AssuranceLevel::Fast => "nvidia_orchestrator-8b@q6_k_l",
            AssuranceLevel::Flash => "glm-4.7-flash",
            AssuranceLevel::Mini => "mistralai/ministral-3-14b-reasoning",
            AssuranceLevel::Full => "glm-4.7",
            AssuranceLevel::FullLowOC => "glm-4.7",
            AssuranceLevel::FullMedOC => "glm-4.7",
            AssuranceLevel::FullHighOC => "glm-4.7",
        }
    }

    /// Get the API endpoint URL for this level's decision model.
    ///
    /// - LM Studio (localhost:1234): Local models (8b, flash, ministral)
    /// - Z.ai API: glm-4.7 (FULL and OC levels)
    /// - OpenRouter: OC feedback models (GPT-5.2, Opus)
    pub fn decision_endpoint(&self) -> &'static str {
        match self {
            AssuranceLevel::Fast | AssuranceLevel::Flash | AssuranceLevel::Mini => {
                "http://localhost:1234/v1"
            }
            AssuranceLevel::Full
            | AssuranceLevel::FullLowOC
            | AssuranceLevel::FullMedOC
            | AssuranceLevel::FullHighOC => {
                // glm-4.7 runs on Z.ai API
                "https://api.z.ai/v1"
            }
        }
    }

    /// Get the API endpoint for feedback models at this level.
    /// Returns (endpoint, requires_api_key).
    pub fn feedback_endpoint(&self, model: &str) -> (&'static str, bool) {
        if model.starts_with("anthropic/") || model.starts_with("openai/") {
            // OpenRouter for cloud OC models
            ("https://openrouter.ai/api/v1", true)
        } else {
            // LM Studio for all local models (glm-4.7, ministral, etc.)
            ("http://localhost:1234/v1", false)
        }
    }

    /// Get models that provide realtime feedback (non-blocking advisory) at this level.
    ///
    /// At Fast, larger models provide feedback while 8b decides.
    /// At Full+, premium cloud models provide feedback.
    pub fn feedback_models(&self) -> Vec<&'static str> {
        match self {
            AssuranceLevel::Fast => {
                // 8b decides, all others give feedback
                vec!["glm-4.7-flash", "mistralai/ministral-3-14b-reasoning", "glm-4.7"]
            }
            AssuranceLevel::Flash => {
                // flash decides, mini and full give feedback
                vec!["mistralai/ministral-3-14b-reasoning", "glm-4.7"]
            }
            AssuranceLevel::Mini => {
                // mini decides, full gives feedback
                vec!["glm-4.7"]
            }
            AssuranceLevel::Full => {
                // glm-4.7 decides, no feedback (it's the top local)
                vec![]
            }
            AssuranceLevel::FullLowOC => {
                // glm-4.7 decides, GPT-5.2 provides feedback (cheaper)
                vec!["openai/gpt-5.2"]
            }
            AssuranceLevel::FullMedOC => {
                // glm-4.7 decides, GPT-5.2 + Opus provide feedback
                vec!["openai/gpt-5.2", "anthropic/claude-opus-4-5"]
            }
            AssuranceLevel::FullHighOC => {
                // glm-4.7 decides, GPT-5.2 + Opus + selective GPT-5.2-Pro
                vec!["openai/gpt-5.2", "anthropic/claude-opus-4-5", "openai/gpt-5.2-pro"]
            }
        }
    }

    /// Get overclock models for this level.
    pub fn overclock_models(&self) -> Vec<OverclockModel> {
        match self {
            AssuranceLevel::Fast => vec![],
            AssuranceLevel::Flash => vec![],
            AssuranceLevel::Mini => vec![],
            AssuranceLevel::Full => vec![],
            AssuranceLevel::FullLowOC => vec![OverclockModel::GPT52],
            AssuranceLevel::FullMedOC => vec![
                OverclockModel::GPT52,
                OverclockModel::ClaudeOpus45,
            ],
            AssuranceLevel::FullHighOC => vec![
                OverclockModel::GPT52,
                OverclockModel::ClaudeOpus45,
                OverclockModel::GPT52Pro,
            ],
        }
    }

    /// Whether this level uses overclock (external API models).
    pub fn is_overclocked(&self) -> bool {
        matches!(
            self,
            AssuranceLevel::FullLowOC | AssuranceLevel::FullMedOC | AssuranceLevel::FullHighOC
        )
    }

    /// Whether this level requires double activation (haptic confirm).
    pub fn requires_double_activation(&self) -> bool {
        matches!(self, AssuranceLevel::FullHighOC)
    }

    /// Whether decisions should block until this level's model confirms.
    pub fn blocks_execution(&self) -> bool {
        // At Fast, we don't block - 8b executes immediately
        // At all other levels, we wait for the decision model
        !matches!(self, AssuranceLevel::Fast)
    }

    /// Move to the next higher assurance level (slower, more assured).
    pub fn increase(self) -> Self {
        match self {
            AssuranceLevel::Fast => AssuranceLevel::Flash,
            AssuranceLevel::Flash => AssuranceLevel::Mini,
            AssuranceLevel::Mini => AssuranceLevel::Full,
            AssuranceLevel::Full => AssuranceLevel::FullLowOC,
            AssuranceLevel::FullLowOC => AssuranceLevel::FullMedOC,
            AssuranceLevel::FullMedOC => AssuranceLevel::FullHighOC,
            AssuranceLevel::FullHighOC => AssuranceLevel::FullHighOC,
        }
    }

    /// Move to the next lower assurance level (faster, less assured).
    pub fn decrease(self) -> Self {
        match self {
            AssuranceLevel::Fast => AssuranceLevel::Fast,
            AssuranceLevel::Flash => AssuranceLevel::Fast,
            AssuranceLevel::Mini => AssuranceLevel::Flash,
            AssuranceLevel::Full => AssuranceLevel::Mini,
            AssuranceLevel::FullLowOC => AssuranceLevel::Full,
            AssuranceLevel::FullMedOC => AssuranceLevel::FullLowOC,
            AssuranceLevel::FullHighOC => AssuranceLevel::FullMedOC,
        }
    }

    /// Get index in the level list (for UI display).
    pub fn index(&self) -> usize {
        Self::ALL.iter().position(|l| l == self).unwrap_or(0)
    }

    /// Get the previous level (for dimmed left display).
    pub fn previous(&self) -> Option<Self> {
        if *self == AssuranceLevel::Fast {
            None
        } else {
            Some(self.decrease())
        }
    }

    /// Get the next level (for dimmed right display).
    pub fn next(&self) -> Option<Self> {
        if *self == AssuranceLevel::FullHighOC {
            None
        } else {
            Some(self.increase())
        }
    }
}

impl Default for AssuranceLevel {
    fn default() -> Self {
        AssuranceLevel::Flash // Default to flash - balanced speed/quality
    }
}

/// Overclock models available via OpenRouter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OverclockModel {
    /// Claude Opus 4.5 via OpenRouter.
    ClaudeOpus45,
    /// GPT-5.2 via OpenRouter.
    GPT52,
    /// GPT-5.2-Pro via OpenRouter (selective questions only).
    GPT52Pro,
}

impl OverclockModel {
    /// Get the OpenRouter model ID.
    pub fn openrouter_id(&self) -> &'static str {
        match self {
            OverclockModel::ClaudeOpus45 => "anthropic/claude-opus-4-5",
            OverclockModel::GPT52 => "openai/gpt-5.2",
            OverclockModel::GPT52Pro => "openai/gpt-5.2-pro",
        }
    }

    /// Get display name.
    pub fn name(&self) -> &'static str {
        match self {
            OverclockModel::ClaudeOpus45 => "Claude Opus 4.5",
            OverclockModel::GPT52 => "GPT-5.2",
            OverclockModel::GPT52Pro => "GPT-5.2-Pro",
        }
    }

    /// Whether this model is for selective/filtered queries only.
    pub fn selective_only(&self) -> bool {
        matches!(self, OverclockModel::GPT52Pro)
    }
}

/// State of the confidence slider in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceSliderState {
    /// Current assurance level.
    pub current: AssuranceLevel,

    /// Whether HIGH OC activation is pending (first d-pad press done).
    pub high_oc_pending: bool,

    /// Timestamp of HIGH OC first press (for timeout).
    pub high_oc_pending_since: Option<u64>,

    /// Whether the slider is currently visible/focused.
    pub visible: bool,
}

impl Default for ConfidenceSliderState {
    fn default() -> Self {
        Self {
            current: AssuranceLevel::default(),
            high_oc_pending: false,
            high_oc_pending_since: None,
            visible: true,
        }
    }
}

impl ConfidenceSliderState {
    /// Handle button press to decrease assurance (Select = faster/less assured).
    pub fn press_decrease(&mut self) -> SliderAction {
        self.high_oc_pending = false;
        self.high_oc_pending_since = None;

        let old = self.current;
        self.current = self.current.decrease();

        if old != self.current {
            // Special haptic when deactivating HIGH OC
            if old == AssuranceLevel::FullHighOC {
                return SliderAction::HighOCDeactivated;
            }
            SliderAction::Changed {
                from: old,
                to: self.current,
            }
        } else {
            SliderAction::AtLimit
        }
    }

    /// Handle button press to increase assurance (Start = slower/more assured).
    pub fn press_increase(&mut self) -> SliderAction {
        let old = self.current;

        // Special handling for HIGH OC double-press requirement
        if self.current == AssuranceLevel::FullMedOC {
            if self.high_oc_pending {
                // Second press - activate HIGH OC
                self.current = AssuranceLevel::FullHighOC;
                self.high_oc_pending = false;
                self.high_oc_pending_since = None;
                return SliderAction::HighOCActivated;
            } else {
                // First press - pending
                self.high_oc_pending = true;
                self.high_oc_pending_since = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                );
                return SliderAction::HighOCPending;
            }
        }

        // Normal increase
        self.high_oc_pending = false;
        self.high_oc_pending_since = None;
        self.current = self.current.increase();

        if old != self.current {
            SliderAction::Changed {
                from: old,
                to: self.current,
            }
        } else {
            SliderAction::AtLimit
        }
    }

    /// Check if HIGH OC pending has timed out (2 second window).
    pub fn check_high_oc_timeout(&mut self) -> bool {
        if let Some(since) = self.high_oc_pending_since {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            if now - since > 2000 {
                self.high_oc_pending = false;
                self.high_oc_pending_since = None;
                return true;
            }
        }
        false
    }

    /// Get the three levels to display (previous, current, next).
    pub fn display_levels(&self) -> (Option<AssuranceLevel>, AssuranceLevel, Option<AssuranceLevel>) {
        (self.current.previous(), self.current, self.current.next())
    }
}

/// Action resulting from slider input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SliderAction {
    /// Level changed.
    Changed {
        from: AssuranceLevel,
        to: AssuranceLevel,
    },
    /// At the limit, can't go further.
    AtLimit,
    /// HIGH OC pending (first press done, waiting for second).
    HighOCPending,
    /// HIGH OC activated (second press done).
    HighOCActivated,
    /// HIGH OC deactivated (stepped down from HIGH OC).
    HighOCDeactivated,
    /// HIGH OC pending timed out.
    HighOCTimeout,
}

impl SliderAction {
    /// Get haptic feedback pattern for this action.
    pub fn haptic_pattern(&self) -> HapticPattern {
        match self {
            SliderAction::Changed { .. } => HapticPattern::Click,
            SliderAction::AtLimit => HapticPattern::Bump,
            SliderAction::HighOCPending => HapticPattern::Tap,
            SliderAction::HighOCActivated => HapticPattern::DoubleClick,
            SliderAction::HighOCDeactivated => HapticPattern::Debump,
            SliderAction::HighOCTimeout => HapticPattern::Debump,
        }
    }
}

/// Haptic feedback patterns for the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HapticPattern {
    /// Single click.
    Click,
    /// Double click.
    DoubleClick,
    /// Light tap (for pending state).
    Tap,
    /// Bump (hit limit).
    Bump,
    /// De-bump (timeout/deactivation).
    Debump,
    /// Role swap notification.
    RoleSwap,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assurance_levels() {
        let level = AssuranceLevel::Fast;
        assert!(!level.blocks_execution());
        assert!(!level.is_overclocked());

        let level = AssuranceLevel::Full;
        assert!(level.blocks_execution());
        assert!(!level.is_overclocked());

        let level = AssuranceLevel::FullLowOC;
        assert!(level.is_overclocked());
        assert_eq!(level.overclock_models().len(), 1);
    }

    #[test]
    fn test_slider_navigation() {
        let mut slider = ConfidenceSliderState::default();
        // Default is Flash

        // Navigate up through levels
        slider.press_increase();
        assert_eq!(slider.current, AssuranceLevel::Mini);

        slider.press_increase();
        assert_eq!(slider.current, AssuranceLevel::Full);

        slider.press_increase();
        assert_eq!(slider.current, AssuranceLevel::FullLowOC);

        // Navigate back down
        slider.press_decrease();
        assert_eq!(slider.current, AssuranceLevel::Full);
    }

    #[test]
    fn test_high_oc_double_press() {
        let mut slider = ConfidenceSliderState {
            current: AssuranceLevel::FullMedOC,
            ..Default::default()
        };

        // First press - pending
        let action = slider.press_increase();
        assert_eq!(action, SliderAction::HighOCPending);
        assert!(slider.high_oc_pending);
        assert_eq!(slider.current, AssuranceLevel::FullMedOC);

        // Second press - activated
        let action = slider.press_increase();
        assert_eq!(action, SliderAction::HighOCActivated);
        assert!(!slider.high_oc_pending);
        assert_eq!(slider.current, AssuranceLevel::FullHighOC);
    }

    #[test]
    fn test_display_levels() {
        let slider = ConfidenceSliderState {
            current: AssuranceLevel::Full,
            ..Default::default()
        };

        let (prev, curr, next) = slider.display_levels();
        assert_eq!(prev, Some(AssuranceLevel::Mini));
        assert_eq!(curr, AssuranceLevel::Full);
        assert_eq!(next, Some(AssuranceLevel::FullLowOC));
    }
}
