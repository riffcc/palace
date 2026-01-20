//! Display profiles for different viewing contexts.
//!
//! Supports different UI scaling for:
//! - Desktop: 42" TV at 1ft distance (close viewing)
//! - Couch: 42" TV at 9ft distance (living room)
//! - Handheld: 6" screen at 1.5ft distance (GPD Win 4)

use serde::{Deserialize, Serialize};

/// Display profile for UI scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DisplayProfile {
    /// Desktop/close viewing - 42" TV at ~1ft
    /// Machine: tealc, no controller initially connected
    #[default]
    Desktop,
    /// Couch/living room - 42" TV at ~9ft
    /// Machine: tealc, controller connected
    Couch,
    /// Handheld - 6" screen at ~1.5ft
    /// Machine: sterling (GPD Win 4)
    Handheld,
}

impl DisplayProfile {
    /// Detect profile based on hostname and controller state.
    pub fn detect(controller_connected: bool) -> Self {
        let hostname = std::env::var("HOSTNAME")
            .or_else(|_| std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()))
            .unwrap_or_default();

        match hostname.to_lowercase().as_str() {
            "sterling" => DisplayProfile::Handheld,
            "tealc" => {
                if controller_connected {
                    DisplayProfile::Couch
                } else {
                    DisplayProfile::Desktop
                }
            }
            _ => {
                // Default based on controller
                if controller_connected {
                    DisplayProfile::Couch
                } else {
                    DisplayProfile::Desktop
                }
            }
        }
    }

    /// Get UI scale factor for this profile.
    pub fn scale(&self) -> f32 {
        match self {
            DisplayProfile::Desktop => 1.0,
            DisplayProfile::Couch => 2.5,   // Much larger for distance viewing
            DisplayProfile::Handheld => 0.7, // Smaller for close handheld
        }
    }

    /// Get base font size for this profile.
    pub fn font_size(&self) -> f32 {
        match self {
            DisplayProfile::Desktop => 16.0,
            DisplayProfile::Couch => 32.0,
            DisplayProfile::Handheld => 12.0,
        }
    }

    /// Get header font size.
    pub fn header_size(&self) -> f32 {
        match self {
            DisplayProfile::Desktop => 20.0,
            DisplayProfile::Couch => 48.0,
            DisplayProfile::Handheld => 14.0,
        }
    }

    /// Get small font size (labels, hints).
    pub fn small_size(&self) -> f32 {
        match self {
            DisplayProfile::Desktop => 12.0,
            DisplayProfile::Couch => 24.0,
            DisplayProfile::Handheld => 10.0,
        }
    }

    /// Get padding in pixels.
    pub fn padding(&self) -> f32 {
        match self {
            DisplayProfile::Desktop => 12.0,
            DisplayProfile::Couch => 30.0,
            DisplayProfile::Handheld => 8.0,
        }
    }

    /// Get line height multiplier.
    pub fn line_height(&self) -> f32 {
        1.3
    }

    /// Profile name for display.
    pub fn name(&self) -> &'static str {
        match self {
            DisplayProfile::Desktop => "Desktop",
            DisplayProfile::Couch => "Couch",
            DisplayProfile::Handheld => "Handheld",
        }
    }
}

/// UI sizing configuration derived from display profile.
#[derive(Debug, Clone)]
pub struct UISizing {
    pub profile: DisplayProfile,
    pub font_size: f32,
    pub header_size: f32,
    pub small_size: f32,
    pub padding: f32,
    pub line_height: f32,
}

impl UISizing {
    /// Create sizing from a display profile.
    pub fn from_profile(profile: DisplayProfile) -> Self {
        Self {
            profile,
            font_size: profile.font_size(),
            header_size: profile.header_size(),
            small_size: profile.small_size(),
            padding: profile.padding(),
            line_height: profile.line_height(),
        }
    }

    /// Update to a new profile.
    pub fn set_profile(&mut self, profile: DisplayProfile) {
        *self = Self::from_profile(profile);
    }
}

impl Default for UISizing {
    fn default() -> Self {
        Self::from_profile(DisplayProfile::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_scaling() {
        assert!(DisplayProfile::Couch.font_size() > DisplayProfile::Desktop.font_size());
        assert!(DisplayProfile::Handheld.font_size() < DisplayProfile::Desktop.font_size());
    }

    #[test]
    fn test_sizing_from_profile() {
        let sizing = UISizing::from_profile(DisplayProfile::Couch);
        assert_eq!(sizing.font_size, 32.0);
    }
}
