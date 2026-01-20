//! Touchpad surface interaction for Conductor (PS5 DualSense).
//!
//! The touchpad acts as a dynamic surface for:
//! - Touch position indicator
//! - Click to select
//! - Fling gestures to dismiss
//! - Pinch to combine ideas
//! - Drag to chain concepts

use serde::{Deserialize, Serialize};

/// A point on the touchpad (normalized 0.0-1.0).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TouchPoint {
    pub x: f32,
    pub y: f32,
}

impl TouchPoint {
    /// Create a new touch point.
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
        }
    }

    /// Distance to another point.
    pub fn distance(&self, other: &TouchPoint) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }

    /// Angle to another point.
    pub fn angle_to(&self, other: &TouchPoint) -> f32 {
        (other.y - self.y).atan2(other.x - self.x)
    }
}

/// Touchpad gesture types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TouchpadGesture {
    /// Simple tap.
    Tap { position: TouchPoint },

    /// Double tap.
    DoubleTap { position: TouchPoint },

    /// Long press.
    LongPress { position: TouchPoint },

    /// Swipe in a direction.
    Swipe {
        start: TouchPoint,
        end: TouchPoint,
        velocity: f32,
    },

    /// Fling (fast swipe to dismiss).
    Fling {
        start: TouchPoint,
        direction: FlingDirection,
        velocity: f32,
    },

    /// Pinch (two fingers moving together/apart).
    Pinch {
        center: TouchPoint,
        scale: f32, // < 1.0 = pinch in, > 1.0 = pinch out
    },

    /// Drag.
    Drag {
        start: TouchPoint,
        current: TouchPoint,
    },

    /// Two-finger drag (for scrolling).
    TwoFingerDrag {
        delta_x: f32,
        delta_y: f32,
    },
}

/// Direction of a fling gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlingDirection {
    Up,
    Down,
    Left,
    Right,
}

impl FlingDirection {
    /// Determine direction from angle.
    pub fn from_angle(angle: f32) -> Self {
        let pi = std::f32::consts::PI;
        if angle > pi * 0.25 && angle <= pi * 0.75 {
            FlingDirection::Up
        } else if angle > pi * 0.75 || angle <= -pi * 0.75 {
            FlingDirection::Left
        } else if angle > -pi * 0.75 && angle <= -pi * 0.25 {
            FlingDirection::Down
        } else {
            FlingDirection::Right
        }
    }
}

/// Current state of the touchpad.
#[derive(Debug, Clone, Default)]
pub struct TouchpadState {
    /// Primary touch point (if touching).
    pub primary_touch: Option<TouchPoint>,

    /// Secondary touch point (for two-finger gestures).
    pub secondary_touch: Option<TouchPoint>,

    /// Whether the touchpad button is pressed.
    pub pressed: bool,

    /// Touch history for gesture detection.
    touch_history: Vec<TouchHistoryEntry>,
}

#[derive(Debug, Clone)]
struct TouchHistoryEntry {
    point: TouchPoint,
    timestamp: std::time::Instant,
    pressed: bool,
}

impl TouchpadState {
    /// Create a new touchpad state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update with new touch data.
    pub fn update(&mut self, x: f32, y: f32, pressed: bool) {
        let point = TouchPoint::new(x, y);
        self.primary_touch = Some(point);
        self.pressed = pressed;

        self.touch_history.push(TouchHistoryEntry {
            point,
            timestamp: std::time::Instant::now(),
            pressed,
        });

        // Keep only recent history (last 500ms)
        let cutoff = std::time::Instant::now() - std::time::Duration::from_millis(500);
        self.touch_history.retain(|e| e.timestamp > cutoff);
    }

    /// Clear touch.
    pub fn clear(&mut self) {
        self.primary_touch = None;
        self.secondary_touch = None;
        self.pressed = false;
    }

    /// Detect gesture from history.
    pub fn detect_gesture(&self) -> Option<TouchpadGesture> {
        if self.touch_history.is_empty() {
            return None;
        }

        let history = &self.touch_history;
        let first = history.first()?;
        let last = history.last()?;

        // Check for tap (short touch, minimal movement)
        if history.len() >= 2 {
            let duration = last.timestamp.duration_since(first.timestamp);
            let distance = first.point.distance(&last.point);

            if duration < std::time::Duration::from_millis(200) && distance < 0.05 {
                return Some(TouchpadGesture::Tap {
                    position: first.point,
                });
            }
        }

        // Check for swipe/fling
        if history.len() >= 3 {
            let distance = first.point.distance(&last.point);
            let duration = last.timestamp.duration_since(first.timestamp);
            let velocity = distance / duration.as_secs_f32().max(0.001);

            if distance > 0.2 {
                let angle = first.point.angle_to(&last.point);

                if velocity > 2.0 {
                    // Fast swipe = fling
                    return Some(TouchpadGesture::Fling {
                        start: first.point,
                        direction: FlingDirection::from_angle(angle),
                        velocity,
                    });
                } else {
                    // Slower = swipe
                    return Some(TouchpadGesture::Swipe {
                        start: first.point,
                        end: last.point,
                        velocity,
                    });
                }
            }
        }

        // Check for long press
        if history.len() >= 2 {
            let duration = last.timestamp.duration_since(first.timestamp);
            let distance = first.point.distance(&last.point);

            if duration > std::time::Duration::from_millis(500) && distance < 0.05 {
                return Some(TouchpadGesture::LongPress {
                    position: first.point,
                });
            }
        }

        None
    }
}

/// The touchpad surface for the UI.
#[derive(Debug)]
pub struct TouchpadSurface {
    /// Current state.
    pub state: TouchpadState,

    /// Items currently on the surface.
    pub items: Vec<SurfaceItem>,

    /// Selected item indices.
    pub selected: Vec<usize>,
}

/// An item on the touchpad surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceItem {
    /// Item ID.
    pub id: String,

    /// Position on surface.
    pub position: TouchPoint,

    /// Size (normalized).
    pub size: f32,

    /// Label.
    pub label: String,

    /// Whether selected.
    pub selected: bool,

    /// Item type.
    pub item_type: SurfaceItemType,
}

/// Types of items on the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceItemType {
    /// An option to select.
    Option,
    /// A decision already made.
    Decision,
    /// A concept/idea.
    Concept,
    /// A combination of items.
    Combined,
}

impl TouchpadSurface {
    /// Create a new touchpad surface.
    pub fn new() -> Self {
        Self {
            state: TouchpadState::new(),
            items: vec![],
            selected: vec![],
        }
    }

    /// Add an item.
    pub fn add_item(&mut self, item: SurfaceItem) {
        self.items.push(item);
    }

    /// Find item at position.
    pub fn item_at(&self, point: TouchPoint) -> Option<usize> {
        for (i, item) in self.items.iter().enumerate() {
            let dist = item.position.distance(&point);
            if dist < item.size {
                return Some(i);
            }
        }
        None
    }

    /// Handle a gesture.
    pub fn handle_gesture(&mut self, gesture: TouchpadGesture) -> Option<SurfaceAction> {
        match gesture {
            TouchpadGesture::Tap { position } => {
                // Select item at position
                if let Some(idx) = self.item_at(position) {
                    self.selected.push(idx);
                    self.items[idx].selected = true;
                    return Some(SurfaceAction::Select(idx));
                }
            }

            TouchpadGesture::DoubleTap { position } => {
                // Confirm selection at position
                if let Some(idx) = self.item_at(position) {
                    return Some(SurfaceAction::Confirm(idx));
                }
            }

            TouchpadGesture::Fling { start, direction, .. } => {
                // Dismiss item
                if let Some(idx) = self.item_at(start) {
                    self.items.remove(idx);
                    self.selected.retain(|&i| i != idx);
                    return Some(SurfaceAction::Dismiss(idx, direction));
                }
            }

            TouchpadGesture::Pinch { center, scale } => {
                if scale < 0.7 && self.selected.len() >= 2 {
                    // Combine selected items
                    let ids: Vec<_> = self
                        .selected
                        .iter()
                        .filter_map(|&i| self.items.get(i))
                        .map(|item| item.id.clone())
                        .collect();

                    let combined = SurfaceItem {
                        id: format!("combined_{}", uuid::Uuid::new_v4()),
                        position: center,
                        size: 0.1,
                        label: format!("Combined ({})", ids.len()),
                        selected: false,
                        item_type: SurfaceItemType::Combined,
                    };

                    // Remove old items and add combined
                    let mut to_remove: Vec<_> = self.selected.clone();
                    to_remove.sort_by(|a, b| b.cmp(a)); // Remove from end first
                    for idx in to_remove {
                        if idx < self.items.len() {
                            self.items.remove(idx);
                        }
                    }
                    self.selected.clear();
                    self.items.push(combined);

                    return Some(SurfaceAction::Combine(ids));
                }
            }

            TouchpadGesture::Drag { start, current } => {
                // Move item
                if let Some(idx) = self.item_at(start) {
                    self.items[idx].position = current;
                    return Some(SurfaceAction::Move(idx, current));
                }
            }

            _ => {}
        }

        None
    }

    /// Clear the surface.
    pub fn clear(&mut self) {
        self.items.clear();
        self.selected.clear();
    }
}

impl Default for TouchpadSurface {
    fn default() -> Self {
        Self::new()
    }
}

/// Action resulting from surface interaction.
#[derive(Debug, Clone)]
pub enum SurfaceAction {
    /// Item selected.
    Select(usize),
    /// Item confirmed.
    Confirm(usize),
    /// Item dismissed.
    Dismiss(usize, FlingDirection),
    /// Items combined.
    Combine(Vec<String>),
    /// Item moved.
    Move(usize, TouchPoint),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_touch_point_distance() {
        let p1 = TouchPoint::new(0.0, 0.0);
        let p2 = TouchPoint::new(0.3, 0.4);
        assert!((p1.distance(&p2) - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_fling_direction() {
        assert_eq!(FlingDirection::from_angle(0.0), FlingDirection::Right);
        assert_eq!(
            FlingDirection::from_angle(std::f32::consts::FRAC_PI_2),
            FlingDirection::Up
        );
        assert_eq!(
            FlingDirection::from_angle(-std::f32::consts::FRAC_PI_2),
            FlingDirection::Down
        );
        assert_eq!(
            FlingDirection::from_angle(std::f32::consts::PI),
            FlingDirection::Left
        );
    }

    #[test]
    fn test_surface_item_at() {
        let mut surface = TouchpadSurface::new();
        surface.add_item(SurfaceItem {
            id: "test".into(),
            position: TouchPoint::new(0.5, 0.5),
            size: 0.1,
            label: "Test".into(),
            selected: false,
            item_type: SurfaceItemType::Option,
        });

        assert!(surface.item_at(TouchPoint::new(0.5, 0.5)).is_some());
        assert!(surface.item_at(TouchPoint::new(0.0, 0.0)).is_none());
    }
}
