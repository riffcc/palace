//! Radial menu system for Conductor.
//!
//! Dual radial menus controlled by thumbsticks while X is held:
//! - Left stick: Topic/Focus (L3 toggles)
//! - Right stick: Intent/Strategy (R3 toggles)

use serde::{Deserialize, Serialize};

/// Which radial menu side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadialMenuSide {
    Left,
    Right,
}

/// Mode for a radial menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RadialMode {
    // Left stick modes
    #[default]
    Topic,
    Focus,

    // Right stick modes
    Intent,
    Method,
    Strategy,
}

impl RadialMode {
    /// Toggle to the next mode for this menu.
    pub fn toggle(self) -> Self {
        match self {
            // Left stick toggles
            RadialMode::Topic => RadialMode::Focus,
            RadialMode::Focus => RadialMode::Topic,

            // Right stick toggles
            RadialMode::Intent => RadialMode::Method,
            RadialMode::Method => RadialMode::Strategy,
            RadialMode::Strategy => RadialMode::Intent,
        }
    }

    /// Get display name.
    pub fn name(&self) -> &'static str {
        match self {
            RadialMode::Topic => "Topic",
            RadialMode::Focus => "Focus",
            RadialMode::Intent => "Intent",
            RadialMode::Method => "Method",
            RadialMode::Strategy => "Strategy",
        }
    }

    /// Get description.
    pub fn description(&self) -> &'static str {
        match self {
            RadialMode::Topic => "What area to work on",
            RadialMode::Focus => "Specific focus within topic",
            RadialMode::Intent => "What you want to achieve",
            RadialMode::Method => "How to approach it",
            RadialMode::Strategy => "Overall strategy/pattern",
        }
    }
}

/// A selection made via the radial menu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadialSelection {
    /// Which menu.
    pub menu: RadialMenuSide,

    /// Current mode.
    pub mode: RadialMode,

    /// Angle in radians (0 = right, PI/2 = up).
    pub angle: f32,

    /// Magnitude (how far the stick is pushed).
    pub magnitude: f32,
}

impl RadialSelection {
    /// Get the sector index (0-7 for 8 sectors).
    pub fn sector(&self, num_sectors: usize) -> usize {
        let normalized = (self.angle + std::f32::consts::PI) / (2.0 * std::f32::consts::PI);
        let sector = (normalized * num_sectors as f32) as usize;
        sector % num_sectors
    }

    /// Check if this is a strong selection (high magnitude).
    pub fn is_strong(&self) -> bool {
        self.magnitude > 0.8
    }
}

/// An item in a radial menu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadialItem {
    /// Item ID.
    pub id: String,

    /// Label.
    pub label: String,

    /// Icon (emoji or path).
    pub icon: Option<String>,

    /// Description.
    pub description: Option<String>,

    /// Whether this item is currently available.
    pub available: bool,

    /// Sub-items (for nested radials).
    pub children: Vec<RadialItem>,
}

impl RadialItem {
    /// Create a new radial item.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            description: None,
            available: true,
            children: vec![],
        }
    }

    /// Set icon.
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Mark as unavailable.
    pub fn unavailable(mut self) -> Self {
        self.available = false;
        self
    }

    /// Add a child item.
    pub fn with_child(mut self, child: RadialItem) -> Self {
        self.children.push(child);
        self
    }
}

/// A radial menu.
#[derive(Debug, Clone)]
pub struct RadialMenu {
    /// Menu side.
    pub side: RadialMenuSide,

    /// Current mode.
    pub mode: RadialMode,

    /// Items in the menu.
    pub items: Vec<RadialItem>,

    /// Selected sector index.
    pub selected_sector: Option<usize>,

    /// Whether the menu is active (visible).
    pub active: bool,
}

impl RadialMenu {
    /// Create a new radial menu.
    pub fn new(side: RadialMenuSide) -> Self {
        Self {
            side,
            mode: match side {
                RadialMenuSide::Left => RadialMode::Topic,
                RadialMenuSide::Right => RadialMode::Intent,
            },
            items: vec![],
            selected_sector: None,
            active: false,
        }
    }

    /// Set items.
    pub fn with_items(mut self, items: Vec<RadialItem>) -> Self {
        self.items = items;
        self
    }

    /// Activate the menu.
    pub fn activate(&mut self) {
        self.active = true;
    }

    /// Deactivate the menu.
    pub fn deactivate(&mut self) {
        self.active = false;
        self.selected_sector = None;
    }

    /// Toggle mode.
    pub fn toggle_mode(&mut self) {
        self.mode = self.mode.toggle();
    }

    /// Update selection based on stick input.
    pub fn update_selection(&mut self, angle: f32, magnitude: f32) {
        if magnitude < 0.3 {
            self.selected_sector = None;
            return;
        }

        let num_sectors = self.items.len().max(1);
        let normalized = (angle + std::f32::consts::PI) / (2.0 * std::f32::consts::PI);
        let sector = (normalized * num_sectors as f32) as usize;
        self.selected_sector = Some(sector % num_sectors);
    }

    /// Get the currently selected item.
    pub fn selected_item(&self) -> Option<&RadialItem> {
        self.selected_sector
            .and_then(|sector| self.items.get(sector))
    }

    /// Get items for current mode.
    pub fn items_for_mode(&self) -> Vec<RadialItem> {
        // In a real implementation, this would be contextually generated
        // based on the current project state and mode
        match self.mode {
            RadialMode::Topic => vec![
                RadialItem::new("code", "Code").with_icon("💻"),
                RadialItem::new("tests", "Tests").with_icon("🧪"),
                RadialItem::new("docs", "Docs").with_icon("📚"),
                RadialItem::new("build", "Build").with_icon("🔨"),
                RadialItem::new("deploy", "Deploy").with_icon("🚀"),
                RadialItem::new("debug", "Debug").with_icon("🐛"),
                RadialItem::new("review", "Review").with_icon("👀"),
                RadialItem::new("plan", "Plan").with_icon("📋"),
            ],
            RadialMode::Focus => vec![
                RadialItem::new("current_file", "Current File"),
                RadialItem::new("current_function", "Current Function"),
                RadialItem::new("related_files", "Related Files"),
                RadialItem::new("entire_crate", "Entire Crate"),
            ],
            RadialMode::Intent => vec![
                RadialItem::new("fix", "Fix").with_icon("🔧"),
                RadialItem::new("add", "Add").with_icon("➕"),
                RadialItem::new("remove", "Remove").with_icon("➖"),
                RadialItem::new("refactor", "Refactor").with_icon("♻️"),
                RadialItem::new("optimize", "Optimize").with_icon("⚡"),
                RadialItem::new("explain", "Explain").with_icon("💬"),
            ],
            RadialMode::Method => vec![
                RadialItem::new("quick", "Quick Fix"),
                RadialItem::new("careful", "Careful Analysis"),
                RadialItem::new("exploratory", "Exploratory"),
                RadialItem::new("systematic", "Systematic"),
            ],
            RadialMode::Strategy => vec![
                RadialItem::new("tdd", "TDD").with_description("Test-Driven Development"),
                RadialItem::new("incremental", "Incremental").with_description("Small steps"),
                RadialItem::new("rewrite", "Rewrite").with_description("Start fresh"),
                RadialItem::new("hybrid", "Hybrid").with_description("Mix approaches"),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radial_mode_toggle() {
        let mode = RadialMode::Topic;
        assert_eq!(mode.toggle(), RadialMode::Focus);
        assert_eq!(RadialMode::Focus.toggle(), RadialMode::Topic);

        let mode = RadialMode::Intent;
        assert_eq!(mode.toggle(), RadialMode::Method);
        assert_eq!(RadialMode::Method.toggle(), RadialMode::Strategy);
        assert_eq!(RadialMode::Strategy.toggle(), RadialMode::Intent);
    }

    #[test]
    fn test_radial_selection_sector() {
        let selection = RadialSelection {
            menu: RadialMenuSide::Left,
            mode: RadialMode::Topic,
            angle: 0.0,
            magnitude: 1.0,
        };

        // With 8 sectors, angle 0 (right) should be sector 4
        assert_eq!(selection.sector(8), 4);
    }

    #[test]
    fn test_radial_menu() {
        let mut menu = RadialMenu::new(RadialMenuSide::Left);
        menu.items = menu.items_for_mode();

        menu.activate();
        assert!(menu.active);

        menu.update_selection(0.0, 0.9);
        assert!(menu.selected_sector.is_some());
    }
}
