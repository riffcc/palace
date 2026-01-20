//! Palace Demo: Dual-screen Pokemon + Orchestrator demo.
//!
//! This example demonstrates the full Palace system:
//! - Left side: GBA emulator running Pokemon
//! - Right side: Orchestrator UI with LLM output
//! - Bottom-right: Confidence slider
//!
//! Controls:
//! - PS/Home button: Toggle focus between Game and Orchestrator
//! - D-pad +/-: Adjust confidence level
//! - Arrow keys: GBA directional input
//! - Z/X: A/B buttons
//! - Enter: Start
//! - Shift: Select
//!
//! Run with:
//! ```
//! cargo run -p palace-render --example demo
//! ```

use mountain::benchmarks::pokebench::{GbaButton, GbaEmulator, PokeBenchConfig};
use palace_render::{DualScreen, FrameBuffer, PalaceRenderer, RenderConfig, UIState};
use std::time::{Duration, Instant};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

/// Application state for the demo.
struct DemoApp {
    /// Window (created on resume).
    window: Option<Window>,

    /// Renderer.
    renderer: Option<PalaceRenderer>,

    /// GBA emulator (mock when feature disabled).
    emulator: GbaEmulator,

    /// UI state.
    ui_state: UIState,

    /// Dual screen configuration.
    dual_screen: DualScreen,

    /// Last frame time.
    last_frame: Instant,

    /// Frame count.
    frame_count: u64,

    /// FPS timer.
    fps_timer: Instant,
    fps_count: u32,
    current_fps: f32,
}

impl DemoApp {
    fn new() -> Self {
        // Create mock GBA emulator
        let config = PokeBenchConfig::default();
        let emulator = GbaEmulator::new(&config).expect("Failed to create emulator");

        Self {
            window: None,
            renderer: None,
            emulator,
            ui_state: UIState::default(),
            dual_screen: DualScreen::default(),
            last_frame: Instant::now(),
            frame_count: 0,
            fps_timer: Instant::now(),
            fps_count: 0,
            current_fps: 0.0,
        }
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

            // Confidence slider controls
            Key::Character(ref c) if c == "=" || c == "+" => {
                if pressed {
                    self.ui_state.slider.state.press_increase();
                    println!(
                        "Confidence: {:?}",
                        self.ui_state.slider.state.current
                    );
                }
            }
            Key::Character(ref c) if c == "-" || c == "_" => {
                if pressed {
                    self.ui_state.slider.state.press_decrease();
                    println!(
                        "Confidence: {:?}",
                        self.ui_state.slider.state.current
                    );
                }
            }

            // Focus toggle (simulates PS button)
            Key::Named(NamedKey::Tab) => {
                if pressed {
                    self.dual_screen.toggle_focus();
                    println!("Focus: {:?}", self.dual_screen.focused);
                }
            }

            // Split ratio adjustment
            Key::Character(ref c) if c == "[" => {
                if pressed {
                    self.dual_screen.adjust_split(-0.05);
                }
            }
            Key::Character(ref c) if c == "]" => {
                if pressed {
                    self.dual_screen.adjust_split(0.05);
                }
            }

            _ => {}
        }
    }

    /// Run one frame of the demo.
    fn update(&mut self) {
        // Run emulator frame
        let _ = self.emulator.run_frame();
        self.frame_count += 1;

        // Update FPS counter
        self.fps_count += 1;
        let fps_elapsed = self.fps_timer.elapsed();
        if fps_elapsed >= Duration::from_secs(1) {
            self.current_fps = self.fps_count as f32 / fps_elapsed.as_secs_f32();
            self.fps_count = 0;
            self.fps_timer = Instant::now();
        }
    }

    /// Render the current frame.
    fn render(&mut self) {
        if let Some(ref mut renderer) = self.renderer {
            // Create test pattern frame buffer (mock emulator has empty screen)
            // In real usage with gba-emulator feature, this would show actual game
            let mut game_buffer = FrameBuffer::gba();

            // Draw an animated gradient pattern for the demo
            for y in 0..160u32 {
                for x in 0..240u32 {
                    let r = ((x + self.frame_count as u32) % 256) as u8;
                    let g = ((y + self.frame_count as u32 / 2) % 256) as u8;
                    let b = ((x + y) % 256) as u8;
                    game_buffer.set_pixel(x, y, r, g, b, 255);
                }
            }

            // Update renderer with current state
            renderer.update_game_texture(&game_buffer);
            renderer.update_ui_state(&self.ui_state);

            // Render frame
            if let Err(e) = renderer.render() {
                eprintln!("Render error: {}", e);
            }
        }

        self.last_frame = Instant::now();
    }
}

impl ApplicationHandler for DemoApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window_attributes = Window::default_attributes()
            .with_title("Palace Demo - Pokemon + Orchestrator")
            .with_inner_size(LogicalSize::new(1280, 720));

        let window = event_loop.create_window(window_attributes).unwrap();

        // Create renderer
        let config = RenderConfig {
            width: 1280,
            height: 720,
            present_mode: wgpu::PresentMode::AutoVsync,
            split_ratio: 0.6,
        };

        match pollster::block_on(PalaceRenderer::new(&window, config)) {
            Ok(renderer) => {
                self.renderer = Some(renderer);
            }
            Err(e) => {
                eprintln!("Failed to create renderer: {:?}", e);
            }
        }

        self.window = Some(window);

        println!("=== Palace Demo ===");
        println!("Controls:");
        println!("  Arrow keys: D-pad");
        println!("  Z: A button");
        println!("  X: B button");
        println!("  Enter: Start");
        println!("  Shift: Select");
        println!("  +/-: Adjust confidence level");
        println!("  Tab: Toggle focus (Game/Orchestrator)");
        println!("  [/]: Adjust split ratio");
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
                println!("Closing...");
                event_loop.exit();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                // ESC to quit
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

                // Request next frame
                if let Some(ref window) = self.window {
                    window.request_redraw();
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Request continuous redraw for animation
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }
}

fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("Starting Palace Demo...");

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = DemoApp::new();
    event_loop.run_app(&mut app).expect("Event loop error");
}
