//! Main application logic for Palace.

use crate::gamepad::{GamepadEvent, GamepadInput};
use crate::inference::LMStudioClient;

use anyhow::Result;
use mountain::{
    AssuranceLevel, Cascade, CascadeBuilder, ControlDecision, LLMOutput, LLMOutputStream,
    LLMOutputType, ModelTier,
};
use palace_gba::{GbaButton, GbaConfig, GbaEmulator};
use palace_render::{DualScreen, FrameBuffer, PalaceRenderer, RenderConfig, UIState};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Fullscreen, Window, WindowId},
};

/// Run play mode with a ROM.
pub fn run_play(
    rom: PathBuf,
    bios: PathBuf,
    turbo: bool,
    lm_studio_url: &str,
) -> Result<()> {
    info!("Starting play mode with ROM: {:?}", rom);

    if !rom.exists() {
        anyhow::bail!("ROM file not found: {:?}", rom);
    }
    if !bios.exists() {
        anyhow::bail!("BIOS file not found: {:?}", bios);
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = PalaceApp::new(rom, bios, turbo, lm_studio_url)?;
    event_loop.run_app(&mut app)?;

    Ok(())
}

/// Run project mode.
pub fn run_project(path: PathBuf, name: Option<String>, lm_studio_url: &str) -> Result<()> {
    info!("Starting project mode at: {:?}", path);

    // TODO: Implement project mode with Director crate
    println!("Project mode not yet implemented.");
    println!("Path: {:?}", path);
    println!("Name: {:?}", name);
    println!("LM Studio: {}", lm_studio_url);

    Ok(())
}

/// LLM output line for display in the Conductor panel.
#[derive(Debug, Clone)]
pub struct LLMOutputLine {
    pub agent_name: String,
    pub text: String,
    pub output_type: LLMOutputType,
    pub timestamp: Instant,
}

impl From<LLMOutput> for LLMOutputLine {
    fn from(output: LLMOutput) -> Self {
        Self {
            agent_name: output.agent_name,
            text: output.text,
            output_type: output.output_type,
            timestamp: Instant::now(),
        }
    }
}

/// Main Palace application.
pub struct PalaceApp {
    // Window
    window: Option<Window>,
    renderer: Option<PalaceRenderer>,
    width: u32,
    height: u32,

    // Emulator
    emulator: GbaEmulator,

    // UI state
    ui_state: UIState,
    dual_screen: DualScreen,

    // Input
    gamepad: Option<GamepadInput>,

    // Inference
    lm_client: LMStudioClient,
    lm_studio_url: String,
    ai_enabled: bool,
    cascade: Option<Cascade>,
    tokio_runtime: tokio::runtime::Runtime,
    last_ai_decision: Instant,

    // LLM output display
    llm_output_rx: Option<broadcast::Receiver<LLMOutput>>,
    llm_output_stream: Arc<LLMOutputStream>,
    llm_output_buffer: VecDeque<LLMOutputLine>,

    // Frame count and timing
    frame_count: u64,
    frame_start: Instant,

    // Turbo mode
    turbo: bool,
    turbo_intensity: f32,  // 0.0 = normal, 1.0 = unlimited
    turbo_locked: bool,    // Locked at specific intensity
    l2_was_pressed: bool,  // Edge detection for L2
}

/// GBA runs at ~59.73 fps, frame time is ~16.74ms
const FRAME_TIME: Duration = Duration::from_micros(16742);

impl PalaceApp {
    /// Create new app with ROM.
    pub fn new(
        rom: PathBuf,
        bios: PathBuf,
        turbo: bool,
        lm_studio_url: &str,
    ) -> Result<Self> {
        let config = GbaConfig {
            rom_path: rom,
            bios_path: bios,
            audio_enabled: true,
            skip_bios: true,
        };

        let emulator = GbaEmulator::new(&config)?;
        info!("GBA emulator initialized with audio: {}", emulator.has_audio());

        let gamepad = match GamepadInput::new() {
            Ok(g) => {
                info!("Gamepad initialized");
                Some(g)
            }
            Err(e) => {
                warn!("Failed to initialize gamepad: {}", e);
                None
            }
        };

        // Create LLM output stream for displaying Mountain's output
        let llm_output_stream = Arc::new(LLMOutputStream::default());
        let llm_output_rx = Some(llm_output_stream.subscribe());

        // Create tokio runtime for async cascade operations
        let tokio_runtime = tokio::runtime::Runtime::new()?;

        Ok(Self {
            window: None,
            renderer: None,
            width: 3840,
            height: 2160,
            emulator,
            ui_state: UIState::default(),
            dual_screen: DualScreen::default(),
            gamepad,
            lm_client: LMStudioClient::new(lm_studio_url),
            lm_studio_url: lm_studio_url.to_string(),
            ai_enabled: false,
            cascade: None,
            tokio_runtime,
            last_ai_decision: Instant::now(),
            llm_output_rx,
            llm_output_stream,
            llm_output_buffer: VecDeque::with_capacity(100),
            frame_count: 0,
            frame_start: Instant::now(),
            turbo,
            turbo_intensity: 0.0,
            turbo_locked: false,
            l2_was_pressed: false,
        })
    }

    /// Handle keyboard input.
    fn handle_key(&mut self, event: KeyEvent) {
        let pressed = event.state == ElementState::Pressed;

        match event.logical_key {
            // GBA controls
            Key::Named(NamedKey::ArrowUp) => {
                if pressed {
                    self.emulator.press_button(GbaButton::Up);
                } else {
                    self.emulator.release_button(GbaButton::Up);
                }
            }
            Key::Named(NamedKey::ArrowDown) => {
                if pressed {
                    self.emulator.press_button(GbaButton::Down);
                } else {
                    self.emulator.release_button(GbaButton::Down);
                }
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if pressed {
                    self.emulator.press_button(GbaButton::Left);
                } else {
                    self.emulator.release_button(GbaButton::Left);
                }
            }
            Key::Named(NamedKey::ArrowRight) => {
                if pressed {
                    self.emulator.press_button(GbaButton::Right);
                } else {
                    self.emulator.release_button(GbaButton::Right);
                }
            }
            Key::Character(ref c) if c == "z" || c == "Z" => {
                if pressed {
                    self.emulator.press_button(GbaButton::A);
                } else {
                    self.emulator.release_button(GbaButton::A);
                }
            }
            Key::Character(ref c) if c == "x" || c == "X" => {
                if pressed {
                    self.emulator.press_button(GbaButton::B);
                } else {
                    self.emulator.release_button(GbaButton::B);
                }
            }
            Key::Named(NamedKey::Enter) => {
                if pressed {
                    self.emulator.press_button(GbaButton::Start);
                } else {
                    self.emulator.release_button(GbaButton::Start);
                }
            }
            Key::Named(NamedKey::Shift) => {
                if pressed {
                    self.emulator.press_button(GbaButton::Select);
                } else {
                    self.emulator.release_button(GbaButton::Select);
                }
            }
            Key::Character(ref c) if c == "a" || c == "A" => {
                if pressed {
                    self.emulator.press_button(GbaButton::L);
                } else {
                    self.emulator.release_button(GbaButton::L);
                }
            }
            Key::Character(ref c) if c == "s" || c == "S" => {
                if pressed {
                    self.emulator.press_button(GbaButton::R);
                } else {
                    self.emulator.release_button(GbaButton::R);
                }
            }

            // Confidence slider controls
            Key::Character(ref c) if c == "=" || c == "+" => {
                if pressed {
                    self.ui_state.slider.state.press_increase();
                    info!("Confidence: {:?}", self.ui_state.slider.state.current);
                }
            }
            Key::Character(ref c) if c == "-" || c == "_" => {
                if pressed {
                    self.ui_state.slider.state.press_decrease();
                    info!("Confidence: {:?}", self.ui_state.slider.state.current);
                }
            }

            // Focus toggle
            Key::Named(NamedKey::Tab) => {
                if pressed {
                    self.dual_screen.toggle_focus();
                    info!("Focus: {:?}", self.dual_screen.focused);
                }
            }

            // AI toggle
            Key::Named(NamedKey::Space) => {
                if pressed {
                    self.ai_enabled = !self.ai_enabled;
                    info!("AI control: {}", if self.ai_enabled { "ON" } else { "OFF" });

                    // Emit a status message to the LLM output stream
                    if self.ai_enabled {
                        self.llm_output_stream.emit(LLMOutput::text(
                            "System",
                            "AI control enabled - Mountain cascade active",
                        ));
                    } else {
                        self.llm_output_stream.emit(LLMOutput::text(
                            "System",
                            "AI control disabled",
                        ));
                    }
                }
            }

            // Turbo toggle
            Key::Character(ref c) if c == "t" || c == "T" => {
                if pressed {
                    self.turbo = !self.turbo;
                    info!("Turbo: {}", if self.turbo { "ON" } else { "OFF" });
                }
            }

            _ => {}
        }
    }

    /// Handle gamepad input.
    fn handle_gamepad(&mut self) {
        if let Some(ref mut gamepad) = self.gamepad {
            // Process special events
            for event in gamepad.poll() {
                match event {
                    GamepadEvent::Connected(_) => {
                        info!("Controller connected");
                        // Update display profile (e.g., switch to Couch mode on tealc)
                        if let Some(ref mut renderer) = self.renderer {
                            renderer.update_display_profile(true);
                        }
                    }
                    GamepadEvent::Disconnected(_) => {
                        info!("Controller disconnected");
                        // Update display profile (e.g., switch to Desktop mode on tealc)
                        if let Some(ref mut renderer) = self.renderer {
                            renderer.update_display_profile(false);
                        }
                    }
                    GamepadEvent::PSPress => {
                        // Single press: ALWAYS toggles focus
                        // In split: just switches focus
                        // In unitasking: switches focus + display
                        self.dual_screen.handle_guide_single_press();
                        info!(
                            "Focus: {:?}, View: {:?}",
                            if self.dual_screen.game_focused() { "Game" } else { "Conductor" },
                            self.dual_screen.view_mode
                        );
                    }
                    GamepadEvent::PSDoublePress => {
                        // Double tap: split↔unitasking toggle
                        self.dual_screen.handle_guide_double_tap();
                        info!("View mode: {:?}", self.dual_screen.view_mode);
                    }
                    GamepadEvent::ConfidenceIncrease => {
                        // Only when orchestrator is focused
                        if !self.dual_screen.game_focused() {
                            self.ui_state.slider.state.press_increase();
                            info!("Confidence: {:?}", self.ui_state.slider.state.current);
                        }
                    }
                    GamepadEvent::ConfidenceDecrease => {
                        // Only when orchestrator is focused
                        if !self.dual_screen.game_focused() {
                            self.ui_state.slider.state.press_decrease();
                            info!("Confidence: {:?}", self.ui_state.slider.state.current);
                        }
                    }
                    // L2/R2 events used for discrete actions when orchestrator focused
                    GamepadEvent::L2Press | GamepadEvent::L2Release |
                    GamepadEvent::R2Press | GamepadEvent::R2Release => {}
                }
            }

            // Route gamepad to GBA when game is focused
            if self.dual_screen.game_focused() {
                // Collect all button states first
                let (a, b, _, _) = gamepad.face_buttons();
                let (up, down, left, right) = gamepad.dpad();
                let (l1, r1) = gamepad.shoulders();
                let (start, select) = gamepad.start_select();
                let (l2_analog, r2_analog) = gamepad.triggers_analog();

                // Now apply to emulator
                self.set_gba_button(GbaButton::A, a);
                self.set_gba_button(GbaButton::B, b);
                self.set_gba_button(GbaButton::Up, up);
                self.set_gba_button(GbaButton::Down, down);
                self.set_gba_button(GbaButton::Left, left);
                self.set_gba_button(GbaButton::Right, right);
                self.set_gba_button(GbaButton::L, l1);
                self.set_gba_button(GbaButton::R, r1);
                self.set_gba_button(GbaButton::Start, start);
                self.set_gba_button(GbaButton::Select, select);

                // Turbo control via analog triggers
                self.handle_turbo(l2_analog, r2_analog);
            }
        }
    }

    /// Handle turbo via analog triggers.
    /// R2 = proportional turbo (0 = normal, 1 = unlimited)
    /// L2 while R2 held = lock at current intensity
    /// L2 again = unlock
    fn handle_turbo(&mut self, l2: f32, r2: f32) {
        const DEADZONE: f32 = 0.01;

        // L2 toggles lock when R2 is held, otherwise unlocks
        if l2 > DEADZONE && !self.l2_was_pressed {
            if r2 > DEADZONE {
                // L2 while R2 held = lock at current intensity
                self.turbo_locked = true;
                self.turbo_intensity = r2;
                info!("Turbo: LOCKED at {:.0}%", r2 * 100.0);
            } else if self.turbo_locked {
                // L2 without R2 = unlock
                self.turbo_locked = false;
                self.turbo = false;
                self.turbo_intensity = 0.0;
                info!("Turbo: UNLOCKED");
            }
        }
        self.l2_was_pressed = l2 > DEADZONE;

        // R2 = proportional turbo (unless locked)
        if !self.turbo_locked {
            if r2 > DEADZONE {
                self.turbo = true;
                self.turbo_intensity = r2;
            } else {
                self.turbo = false;
                self.turbo_intensity = 0.0;
            }
        }
    }

    /// Set GBA button state (press or release).
    fn set_gba_button(&mut self, button: GbaButton, pressed: bool) {
        if pressed {
            self.emulator.press_button(button);
        } else {
            self.emulator.release_button(button);
        }
    }

    /// Create a cascade based on current confidence level.
    fn create_cascade(&self) -> Cascade {
        let level = self.ui_state.slider.state.current;
        let model_name = level.decision_model();
        let endpoint = level.decision_endpoint();

        // Create a single-tier cascade with the decision model
        // Endpoint is determined by model type (LM Studio, Z.ai, or OpenRouter)
        let tier = ModelTier::local(model_name, 500)
            .with_endpoint(endpoint)
            .with_max_tokens(256)
            .with_temperature(0.3)  // Lower temperature for more deterministic decisions
            .with_priority(1);

        CascadeBuilder::new()
            .add_tier(tier)
            .with_output_stream(self.llm_output_stream.clone())
            .build()
    }

    /// Capture current game state for the cascade.
    fn capture_game_state(&self) -> mountain::StateSnapshot {
        use base64::Engine;

        let screen = self.emulator.screen_rgba();

        // Create a 120x80 thumbnail (half resolution) for better quality while staying efficient
        let thumb_width = 120usize;
        let thumb_height = 80usize;

        // Build raw RGBA data for the thumbnail
        let mut rgba_data = Vec::with_capacity(thumb_width * thumb_height * 4);
        for y in 0..thumb_height {
            for x in 0..thumb_width {
                // Sample from original (2x downscale)
                let src_x = x * 2;
                let src_y = y * 2;
                let idx = src_y * 240 + src_x;
                let pixel = screen[idx];
                rgba_data.push(((pixel >> 16) & 0xFF) as u8); // R
                rgba_data.push(((pixel >> 8) & 0xFF) as u8);  // G
                rgba_data.push((pixel & 0xFF) as u8);         // B
                rgba_data.push(255u8);                         // A
            }
        }

        // Encode as PNG
        let mut png_data = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_data, thumb_width as u32, thumb_height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            if let Ok(mut writer) = encoder.write_header() {
                let _ = writer.write_image_data(&rgba_data);
            }
        }

        // Base64 encode the PNG
        let png_b64 = base64::engine::general_purpose::STANDARD.encode(&png_data);

        mountain::StateSnapshot::new()
            .with_variable("frame", serde_json::json!(self.frame_count))
            .with_variable("screen_thumbnail_b64", serde_json::json!(png_b64))
            .with_variable("screen_width", serde_json::json!(thumb_width))
            .with_variable("screen_height", serde_json::json!(thumb_height))
    }

    /// Apply a control decision to the emulator.
    fn apply_decision(&mut self, decision: &ControlDecision) {
        let button_name = decision.params
            .get("button")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let button = match button_name.to_lowercase().as_str() {
            "a" => Some(GbaButton::A),
            "b" => Some(GbaButton::B),
            "start" => Some(GbaButton::Start),
            "select" => Some(GbaButton::Select),
            "up" => Some(GbaButton::Up),
            "down" => Some(GbaButton::Down),
            "left" => Some(GbaButton::Left),
            "right" => Some(GbaButton::Right),
            "l" | "l1" => Some(GbaButton::L),
            "r" | "r1" => Some(GbaButton::R),
            _ => None,
        };

        match decision.action.as_str() {
            "press" | "tap" => {
                if let Some(btn) = button {
                    debug!("AI: pressing {:?}", btn);
                    self.emulator.press_button(btn);
                    // For tap, we'll release on the next frame
                }
            }
            "release" => {
                if let Some(btn) = button {
                    debug!("AI: releasing {:?}", btn);
                    self.emulator.release_button(btn);
                }
            }
            "wait" | "continue" => {
                // No action
            }
            _ => {
                debug!("AI: unknown action {}", decision.action);
            }
        }
    }

    /// Run one frame of emulation with proper timing.
    fn update(&mut self) {
        self.frame_start = Instant::now();

        self.handle_gamepad();
        self.emulator.run_frame();
        self.frame_count += 1;

        // Poll for LLM outputs and add to buffer
        self.poll_llm_outputs();

        // If AI is enabled, run cascade periodically (every 2 seconds to avoid overwhelming)
        if self.ai_enabled && self.last_ai_decision.elapsed() > Duration::from_secs(2) {
            self.last_ai_decision = Instant::now();

            // Create/update cascade if needed
            let cascade = self.create_cascade();

            // Capture state
            let state = self.capture_game_state();

            // Run cascade asynchronously
            let result = self.tokio_runtime.block_on(cascade.run(&state));

            match result {
                Ok(cascade_result) => {
                    // Apply the final decision
                    self.apply_decision(&cascade_result.final_decision);

                    debug!(
                        "AI cascade completed in {:?}, action: {}",
                        cascade_result.total_duration,
                        cascade_result.final_decision.action
                    );
                }
                Err(e) => {
                    warn!("AI cascade failed: {}", e);
                }
            }
        }
    }

    /// Poll for LLM outputs and add them to the buffer.
    fn poll_llm_outputs(&mut self) {
        if let Some(ref mut rx) = self.llm_output_rx {
            // Drain all available outputs
            while let Ok(output) = rx.try_recv() {
                let line = LLMOutputLine::from(output);

                // Add to buffer (keeping max 50 lines)
                self.llm_output_buffer.push_back(line);
                while self.llm_output_buffer.len() > 50 {
                    self.llm_output_buffer.pop_front();
                }
            }
        }
    }

    /// Sleep to maintain frame rate.
    /// Turbo intensity 0.0 = normal speed, 1.0 = no frame limit (max speed).
    fn frame_limit(&self) {
        if self.turbo && self.turbo_intensity > 0.0 {
            // Scale frame time based on intensity
            // intensity 1.0 = 0% of normal frame time (max speed)
            // intensity 0.5 = 50% of normal frame time (2x speed)
            let scale = 1.0 - self.turbo_intensity;
            if scale > 0.01 {
                let target = FRAME_TIME.mul_f32(scale);
                let elapsed = self.frame_start.elapsed();
                if elapsed < target {
                    spin_sleep::sleep(target - elapsed);
                }
            }
            // else: no sleep, run as fast as possible
        } else {
            let elapsed = self.frame_start.elapsed();
            if elapsed < FRAME_TIME {
                spin_sleep::sleep(FRAME_TIME - elapsed);
            }
        }
    }

    /// Render the current frame.
    fn render(&mut self) {
        if let Some(ref mut renderer) = self.renderer {
            // Sync view mode with renderer
            renderer.sync_dual_screen(&self.dual_screen);

            // Only render game if visible
            if self.dual_screen.show_game() {
                let mut game_buffer = FrameBuffer::gba();

                // Get actual emulator screen output
                let screen = self.emulator.screen_rgba();
                for y in 0..160u32 {
                    for x in 0..240u32 {
                        let idx = (y * 240 + x) as usize;
                        let pixel = screen[idx];
                        // Format is 0x00RRGGBB
                        let r = ((pixel >> 16) & 0xFF) as u8;
                        let g = ((pixel >> 8) & 0xFF) as u8;
                        let b = (pixel & 0xFF) as u8;
                        game_buffer.set_pixel(x, y, r, g, b, 255);
                    }
                }

                renderer.update_game_texture(&game_buffer);
            }

            // Update UI state with LLM output - convert to OutputLine format
            self.ui_state.llm_output = self
                .llm_output_buffer
                .iter()
                .map(|line| {
                    use palace_render::OutputType;
                    let output_type = match line.output_type {
                        LLMOutputType::Thinking => OutputType::Thinking,
                        LLMOutputType::ToolCall | LLMOutputType::ToolResult => OutputType::Tool,
                        LLMOutputType::Text => OutputType::Text,
                        LLMOutputType::Decision => OutputType::Decision,
                        LLMOutputType::Error => OutputType::Error,
                    };
                    palace_render::OutputLine {
                        agent_id: 0,
                        agent_name: line.agent_name.clone(),
                        text: line.text.clone(),
                        output_type,
                        timestamp_ms: 0,
                    }
                })
                .collect();

            // Update AI enabled status in UI
            self.ui_state.ai_enabled = self.ai_enabled;

            renderer.update_ui_state(&self.ui_state);

            // Render
            if let Err(e) = renderer.render() {
                warn!("Render error: {}", e);
            }
        }
    }
}

impl ApplicationHandler for PalaceApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = Window::default_attributes()
            .with_title("Palace - GBA")
            .with_fullscreen(Some(Fullscreen::Borderless(None)));

        let window = event_loop.create_window(window_attributes).unwrap();

        // Create renderer - use Fifo for proper vsync
        let config = RenderConfig {
            width: self.width,
            height: self.height,
            present_mode: wgpu::PresentMode::Fifo, // VSync for smooth 60fps
            split_ratio: 0.6,
        };

        // Check if controller is connected for display profile detection
        let controller_connected = self.gamepad.as_ref().map(|g| g.is_connected()).unwrap_or(false);

        match pollster::block_on(PalaceRenderer::new(&window, config)) {
            Ok(mut renderer) => {
                // Initialize display profile based on machine + controller
                renderer.update_display_profile(controller_connected);
                info!("Display profile: {}", renderer.display_profile().name());
                self.renderer = Some(renderer);
            }
            Err(e) => {
                warn!("Failed to create renderer: {:?}", e);
            }
        }

        self.window = Some(window);

        // Print controls
        println!();
        println!("=== Palace GBA ===");
        println!();
        println!("Controls:");
        println!("  Arrows      D-pad");
        println!("  Z/X         A/B");
        println!("  A/S         L/R");
        println!("  Enter       Start");
        println!("  Shift       Select");
        println!("  T           Turbo toggle");
        println!("  Tab         Toggle focus");
        println!("  Space       Toggle AI");
        println!("  ESC         Quit");
        println!();
        println!("Gamepad (game focused):");
        println!("  A/B/X/Y     A/B/-/-");
        println!("  D-pad       D-pad");
        println!("  L1/R1       L/R");
        println!("  Start/Sel   Start/Select");
        println!("  R2          Turbo (proportional)");
        println!("  L2+R2       Lock turbo at intensity");
        println!("  L2          Unlock turbo");
        println!();
        println!("Gamepad (conductor focused):");
        println!("  Start       Confidence + (slower/more assured)");
        println!("  Select      Confidence - (faster/less assured)");
        println!("  L2/R2       Prev/Next agent");
        println!();
        println!("View controls:");
        println!("  Guide       Toggle focus (split) / Switch view (unitasking)");
        println!("  2x Guide    Split <-> Unitasking");
        println!();

        if let Some(ref gamepad) = self.gamepad {
            if gamepad.is_connected() {
                println!("Gamepad: Connected");
            } else {
                println!("Gamepad: Not connected");
            }
        }

        if let Some(ref renderer) = self.renderer {
            println!("Display: {} mode", renderer.display_profile().name());
        }
        println!("Audio: {}", if self.emulator.has_audio() { "ON" } else { "OFF" });
        println!("Turbo: {}", if self.turbo { "ON" } else { "OFF" });
        println!();
        println!("Palace running - check the window");
        println!();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                info!("Window closed");
                event_loop.exit();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.logical_key == Key::Named(NamedKey::Escape)
                    && event.state == ElementState::Pressed
                {
                    event_loop.exit();
                    return;
                }
                self.handle_key(event);
            }

            WindowEvent::Resized(size) => {
                if let Some(ref mut renderer) = self.renderer {
                    renderer.resize(size);
                }
            }

            WindowEvent::RedrawRequested => {
                self.update();
                self.render();
                self.frame_limit();

                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }
}
