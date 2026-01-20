//! Remote mode and multiplayer coordination for Conductor.
//!
//! Allows Palace to run across multiple devices:
//! - Remote copy controls a program on one device
//! - Controller input and agent output on another device
//! - PS button quick press: switch between controlling app and program
//! - PS button double press: move execution locally, sync git repo
//! - Multiplayer: two users can control Game and Orchestrator, swap roles

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;

/// A Palace node in the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PalaceNode {
    /// Node identifier.
    pub id: String,

    /// Human-readable name.
    pub name: String,

    /// Network address.
    pub address: SocketAddr,

    /// Node capabilities.
    pub capabilities: NodeCapabilities,

    /// Whether this node is currently connected.
    pub connected: bool,

    /// Current role in the session.
    pub role: NodeRole,

    /// Last heartbeat timestamp.
    pub last_heartbeat_ms: u64,
}

/// Capabilities of a Palace node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapabilities {
    /// Has display output (TV, monitor).
    pub has_display: bool,

    /// Has LM Studio or local inference.
    pub has_local_inference: bool,

    /// Has gamepad input.
    pub has_gamepad: bool,

    /// Has touchscreen.
    pub has_touchscreen: bool,

    /// Has pen/stylus input (Wacom, Apple Pencil, etc.).
    pub has_pen_input: bool,

    /// Display type.
    pub display_type: DisplayType,

    /// Available VRAM in GB.
    pub vram_gb: Option<f32>,

    /// GPU name.
    pub gpu_name: Option<String>,

    /// Is a handheld device.
    pub is_handheld: bool,

    /// Platform/OS.
    pub platform: Platform,

    /// CPU architecture.
    pub arch: Architecture,

    /// Number of CPU cores.
    pub cpu_cores: Option<u32>,

    /// RAM in GB.
    pub ram_gb: Option<u32>,

    /// Can run Android emulator efficiently.
    pub can_run_android_emulator: bool,
}

impl Default for NodeCapabilities {
    fn default() -> Self {
        Self {
            has_display: false,
            has_local_inference: false,
            has_gamepad: false,
            has_touchscreen: false,
            has_pen_input: false,
            display_type: DisplayType::LCD,
            vram_gb: None,
            gpu_name: None,
            is_handheld: false,
            platform: Platform::current(),
            arch: Architecture::current(),
            cpu_cores: None,
            ram_gb: None,
            can_run_android_emulator: false,
        }
    }
}

/// Display type for devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DisplayType {
    /// Standard LCD monitor.
    #[default]
    LCD,
    /// OLED display.
    OLED,
    /// E-ink / e-paper (greyscale, low refresh).
    EInk,
    /// Reflective LCD (like Daylight DC-1).
    ReflectiveLCD,
    /// No display (headless server).
    None,
}

/// Role of a node in the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NodeRole {
    /// Not assigned.
    #[default]
    Unassigned,

    /// Running the Game/Program.
    GameHost,

    /// Running the Orchestrator/Agent.
    OrchestratorHost,

    /// Displaying output (TV mode).
    DisplayOnly,

    /// Controller input only.
    ControllerOnly,

    /// Full local mode (both Game and Orchestrator).
    Local,
}

/// Control focus - what the user is currently controlling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ControlFocus {
    /// Controlling the Game/Program (left side of screen).
    #[default]
    Game,

    /// Controlling the Orchestrator/Conductor (right side of screen).
    Orchestrator,
}

impl ControlFocus {
    /// Toggle focus.
    pub fn toggle(self) -> Self {
        match self {
            ControlFocus::Game => ControlFocus::Orchestrator,
            ControlFocus::Orchestrator => ControlFocus::Game,
        }
    }
}

/// A remote session connecting multiple Palace nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSession {
    /// Session identifier.
    pub id: String,

    /// All nodes in the session.
    pub nodes: HashMap<String, PalaceNode>,

    /// This node's ID.
    pub local_node_id: String,

    /// Current control focus.
    pub focus: ControlFocus,

    /// Whether multiplayer mode is enabled.
    pub multiplayer: bool,

    /// Multiplayer settings.
    pub multiplayer_settings: MultiplayerSettings,

    /// Git repository path for syncing.
    pub repo_path: Option<String>,

    /// Project being worked on.
    pub project_name: Option<String>,
}

impl RemoteSession {
    /// Create a new local-only session.
    pub fn local(node_name: impl Into<String>) -> Self {
        let node_id = uuid::Uuid::new_v4().to_string();
        let node = PalaceNode {
            id: node_id.clone(),
            name: node_name.into(),
            address: "127.0.0.1:0".parse().unwrap(),
            capabilities: NodeCapabilities::default(),
            connected: true,
            role: NodeRole::Local,
            last_heartbeat_ms: 0,
        };

        let mut nodes = HashMap::new();
        nodes.insert(node_id.clone(), node);

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            nodes,
            local_node_id: node_id,
            focus: ControlFocus::default(),
            multiplayer: false,
            multiplayer_settings: MultiplayerSettings::default(),
            repo_path: None,
            project_name: None,
        }
    }

    /// Create a remote session connecting to another node.
    pub fn remote(
        local_name: impl Into<String>,
        remote_name: impl Into<String>,
        remote_addr: SocketAddr,
    ) -> Self {
        let local_id = uuid::Uuid::new_v4().to_string();
        let remote_id = uuid::Uuid::new_v4().to_string();

        let local_node = PalaceNode {
            id: local_id.clone(),
            name: local_name.into(),
            address: "127.0.0.1:0".parse().unwrap(),
            capabilities: NodeCapabilities::default(),
            connected: true,
            role: NodeRole::ControllerOnly,
            last_heartbeat_ms: 0,
        };

        let remote_node = PalaceNode {
            id: remote_id.clone(),
            name: remote_name.into(),
            address: remote_addr,
            capabilities: NodeCapabilities::default(),
            connected: false,
            role: NodeRole::GameHost,
            last_heartbeat_ms: 0,
        };

        let mut nodes = HashMap::new();
        nodes.insert(local_id.clone(), local_node);
        nodes.insert(remote_id, remote_node);

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            nodes,
            local_node_id: local_id,
            focus: ControlFocus::default(),
            multiplayer: false,
            multiplayer_settings: MultiplayerSettings::default(),
            repo_path: None,
            project_name: None,
        }
    }

    /// Get the local node.
    pub fn local_node(&self) -> Option<&PalaceNode> {
        self.nodes.get(&self.local_node_id)
    }

    /// Get the local node mutably.
    pub fn local_node_mut(&mut self) -> Option<&mut PalaceNode> {
        self.nodes.get_mut(&self.local_node_id)
    }

    /// Handle PS button quick press - toggle control focus.
    pub fn ps_button_quick_press(&mut self) -> RemoteAction {
        self.focus = self.focus.toggle();
        RemoteAction::FocusChanged(self.focus)
    }

    /// Handle PS button double press - move execution locally.
    pub fn ps_button_double_press(&mut self) -> RemoteAction {
        // In a real implementation, this would:
        // 1. Sync git repo between devices
        // 2. Transfer execution state
        // 3. Swap which node runs what

        if let Some(local) = self.local_node_mut() {
            match local.role {
                NodeRole::ControllerOnly => {
                    local.role = NodeRole::Local;
                    RemoteAction::ExecutionMovedLocal
                }
                NodeRole::Local => {
                    // Already local, swap to remote
                    local.role = NodeRole::ControllerOnly;
                    RemoteAction::ExecutionMovedRemote
                }
                _ => RemoteAction::NoChange,
            }
        } else {
            RemoteAction::NoChange
        }
    }

    /// Enable multiplayer mode.
    pub fn enable_multiplayer(&mut self, settings: MultiplayerSettings) {
        self.multiplayer = true;
        self.multiplayer_settings = settings;
    }

    /// Request a role swap in multiplayer.
    pub fn request_role_swap(&mut self, requester_id: &str) -> SwapRequest {
        if !self.multiplayer {
            return SwapRequest::NotInMultiplayer;
        }

        if self.multiplayer_settings.coop_mode {
            // Instant swap in coop mode
            self.perform_role_swap();
            SwapRequest::InstantSwap
        } else {
            // Needs vote from other player
            SwapRequest::VotePending { requester: requester_id.to_string() }
        }
    }

    /// Accept a role swap vote.
    pub fn accept_swap_vote(&mut self) -> bool {
        if self.multiplayer {
            self.perform_role_swap();
            true
        } else {
            false
        }
    }

    /// Perform the actual role swap.
    fn perform_role_swap(&mut self) {
        // Swap GameHost and OrchestratorHost roles
        for node in self.nodes.values_mut() {
            node.role = match node.role {
                NodeRole::GameHost => NodeRole::OrchestratorHost,
                NodeRole::OrchestratorHost => NodeRole::GameHost,
                other => other,
            };
        }
    }
}

/// Multiplayer settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MultiplayerSettings {
    /// Coop mode - instant role swapping without voting.
    pub coop_mode: bool,

    /// Allow multiple controllers for Game.
    pub multi_controller_game: bool,

    /// Voice chat enabled.
    pub voice_chat: bool,

    /// Shared cursor/pointer.
    pub shared_pointer: bool,
}

/// Action resulting from remote input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteAction {
    /// Control focus changed.
    FocusChanged(ControlFocus),

    /// Execution moved to local device.
    ExecutionMovedLocal,

    /// Execution moved to remote device.
    ExecutionMovedRemote,

    /// No change.
    NoChange,

    /// Role swap occurred.
    RoleSwapped,
}

impl RemoteAction {
    /// Get haptic pattern for this action.
    pub fn haptic_pattern(&self) -> Option<super::gamepad::HapticFeedback> {
        match self {
            RemoteAction::FocusChanged(_) => Some(super::gamepad::HapticFeedback::Click),
            RemoteAction::ExecutionMovedLocal => Some(super::gamepad::HapticFeedback::DoubleClick),
            RemoteAction::ExecutionMovedRemote => Some(super::gamepad::HapticFeedback::DoubleClick),
            RemoteAction::RoleSwapped => Some(super::gamepad::HapticFeedback::RoleSwap),
            RemoteAction::NoChange => None,
        }
    }
}

/// Result of a swap request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapRequest {
    /// Not in multiplayer mode.
    NotInMultiplayer,

    /// Swap happened instantly (coop mode).
    InstantSwap,

    /// Vote pending from other player.
    VotePending { requester: String },

    /// Vote declined.
    Declined,
}

/// Known Palace nodes for easy setup.
pub struct KnownNodes;

impl KnownNodes {
    /// Tealc - the main workstation with LM Studio (localhost).
    /// 16 Zen 5 cores, 128GB unified memory, AMD Ryzen AI MAX+ Pro 395.
    pub fn tealc() -> PalaceNode {
        PalaceNode {
            id: "tealc".into(),
            name: "Tealc".into(),
            address: "10.7.1.135:7777".parse().unwrap(),
            capabilities: NodeCapabilities {
                has_display: true,
                has_local_inference: true,
                has_gamepad: false,
                has_touchscreen: false,
                has_pen_input: false,
                display_type: DisplayType::LCD,
                vram_gb: Some(128.0),
                gpu_name: Some("AMD Ryzen AI MAX+ Pro 395".into()),
                is_handheld: false,
                platform: Platform::Linux,
                arch: Architecture::X86_64,
                cpu_cores: Some(16),
                ram_gb: Some(128),
                can_run_android_emulator: true,
            },
            connected: false,
            role: NodeRole::Unassigned,
            last_heartbeat_ms: 0,
        }
    }

    /// Sterling - GPD Win 4 handheld.
    /// Compute-constrained, good for testing on lower-end hardware.
    pub fn sterling() -> PalaceNode {
        PalaceNode {
            id: "sterling".into(),
            name: "Sterling (GPD Win 4)".into(),
            address: "10.7.1.151:7777".parse().unwrap(),
            capabilities: NodeCapabilities {
                has_display: true,
                has_local_inference: false,
                has_gamepad: true,
                has_touchscreen: true,
                has_pen_input: false,
                display_type: DisplayType::LCD,
                vram_gb: None,
                gpu_name: Some("AMD Radeon 780M".into()),
                is_handheld: true,
                platform: Platform::Linux,
                arch: Architecture::X86_64,
                cpu_cores: Some(8),
                ram_gb: Some(32),
                can_run_android_emulator: false,
            },
            connected: false,
            role: NodeRole::Unassigned,
            last_heartbeat_ms: 0,
        }
    }

    /// MacBook Air M2 - ARM64 testing.
    /// Tests ARM64 compatibility and can run Android emulator.
    pub fn macbook_air_m2() -> PalaceNode {
        PalaceNode {
            id: "macbook-air".into(),
            name: "MacBook Air M2".into(),
            address: "0.0.0.0:7777".parse().unwrap(), // Needs configuration
            capabilities: NodeCapabilities {
                has_display: true,
                has_local_inference: false, // Could run small models
                has_gamepad: false,
                has_touchscreen: false,
                has_pen_input: false,
                display_type: DisplayType::LCD,
                vram_gb: Some(24.0), // Unified memory
                gpu_name: Some("Apple M2".into()),
                is_handheld: false,
                platform: Platform::MacOS,
                arch: Architecture::ARM64,
                cpu_cores: Some(8),
                ram_gb: Some(24),
                can_run_android_emulator: true, // ARM64 Android emulator runs well
            },
            connected: false,
            role: NodeRole::Unassigned,
            last_heartbeat_ms: 0,
        }
    }

    /// Daylight Computer DC-1 - reflective greyscale tablet with Wacom pen.
    /// Android device with unique input modalities: pen, touch, and LivePaper display.
    /// "Third surface" for annotation, sketching, and document work.
    pub fn daylight_dc1() -> PalaceNode {
        PalaceNode {
            id: "daylight".into(),
            name: "Daylight DC-1".into(),
            address: "0.0.0.0:7777".parse().unwrap(), // Needs configuration
            capabilities: NodeCapabilities {
                has_display: true,
                has_local_inference: false,
                has_gamepad: false,
                has_touchscreen: true,
                has_pen_input: true, // Wacom EMR pen
                display_type: DisplayType::ReflectiveLCD, // LivePaper, greyscale
                vram_gb: None,
                gpu_name: None,
                is_handheld: true,
                platform: Platform::Android,
                arch: Architecture::ARM64,
                cpu_cores: Some(8), // Qualcomm Snapdragon
                ram_gb: Some(8),
                can_run_android_emulator: false, // Already Android
            },
            connected: false,
            role: NodeRole::Unassigned,
            last_heartbeat_ms: 0,
        }
    }

    /// Get all known nodes.
    pub fn all() -> Vec<PalaceNode> {
        vec![
            Self::tealc(),
            Self::sterling(),
            Self::macbook_air_m2(),
            Self::daylight_dc1(),
        ]
    }
}

/// Platform/OS type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    Linux,
    MacOS,
    Windows,
    Android,
    IOS,
}

impl Platform {
    /// Detect current platform.
    pub fn current() -> Self {
        #[cfg(target_os = "linux")]
        return Platform::Linux;
        #[cfg(target_os = "macos")]
        return Platform::MacOS;
        #[cfg(target_os = "windows")]
        return Platform::Windows;
        #[cfg(target_os = "android")]
        return Platform::Android;
        #[cfg(target_os = "ios")]
        return Platform::IOS;
        #[cfg(not(any(
            target_os = "linux",
            target_os = "macos",
            target_os = "windows",
            target_os = "android",
            target_os = "ios"
        )))]
        Platform::Linux // Default
    }
}

/// CPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Architecture {
    X86_64,
    ARM64,
    RISCV64,
}

impl Architecture {
    /// Detect current architecture.
    pub fn current() -> Self {
        #[cfg(target_arch = "x86_64")]
        return Architecture::X86_64;
        #[cfg(target_arch = "aarch64")]
        return Architecture::ARM64;
        #[cfg(target_arch = "riscv64")]
        return Architecture::RISCV64;
        #[cfg(not(any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "riscv64"
        )))]
        Architecture::X86_64 // Default
    }
}

/// Dual-screen layout configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualScreenLayout {
    /// Left side width ratio (0.0 - 1.0).
    pub left_ratio: f32,

    /// Whether to show divider.
    pub show_divider: bool,

    /// Left side content.
    pub left_content: ScreenContent,

    /// Right side content.
    pub right_content: ScreenContent,

    /// Current focused side.
    pub focused: ControlFocus,
}

impl Default for DualScreenLayout {
    fn default() -> Self {
        Self {
            left_ratio: 0.6, // Slightly more space for game
            show_divider: true,
            left_content: ScreenContent::Game,
            right_content: ScreenContent::Orchestrator,
            focused: ControlFocus::Game,
        }
    }
}

/// What content is displayed on a screen side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenContent {
    /// The game/program being controlled.
    Game,

    /// The orchestrator/conductor interface.
    Orchestrator,

    /// LLM output stream.
    LLMStream,

    /// Code being generated.
    CodeView,

    /// Browser via Playwright.
    Browser,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_session() {
        let session = RemoteSession::local("test");
        assert!(session.local_node().is_some());
        assert_eq!(session.local_node().unwrap().role, NodeRole::Local);
    }

    #[test]
    fn test_focus_toggle() {
        let mut session = RemoteSession::local("test");
        assert_eq!(session.focus, ControlFocus::Game);

        session.ps_button_quick_press();
        assert_eq!(session.focus, ControlFocus::Orchestrator);

        session.ps_button_quick_press();
        assert_eq!(session.focus, ControlFocus::Game);
    }

    #[test]
    fn test_multiplayer_swap() {
        let mut session = RemoteSession::local("test");
        session.enable_multiplayer(MultiplayerSettings {
            coop_mode: true,
            ..Default::default()
        });

        // Add a second node
        session.nodes.insert(
            "player2".into(),
            PalaceNode {
                id: "player2".into(),
                name: "Player 2".into(),
                address: "127.0.0.1:0".parse().unwrap(),
                capabilities: NodeCapabilities::default(),
                connected: true,
                role: NodeRole::OrchestratorHost,
                last_heartbeat_ms: 0,
            },
        );

        if let Some(local) = session.local_node_mut() {
            local.role = NodeRole::GameHost;
        }

        // In coop mode, swap is instant
        let result = session.request_role_swap("player2");
        assert_eq!(result, SwapRequest::InstantSwap);
    }

    #[test]
    fn test_known_nodes() {
        let tealc = KnownNodes::tealc();
        assert!(tealc.capabilities.has_local_inference);
        assert!(tealc.capabilities.vram_gb.is_some());

        let sterling = KnownNodes::sterling();
        assert!(sterling.capabilities.is_handheld);
        assert!(sterling.capabilities.has_gamepad);
    }
}
