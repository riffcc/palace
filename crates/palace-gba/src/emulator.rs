//! GBA emulator wrapper.

use crate::audio::{create_audio, AudioPlayer};
use crate::error::{GbaError, GbaResult};
use crate::{GBA_HEIGHT, GBA_WIDTH};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use bit::BitIndex;
use rustboyadvance_core::{
    cartridge::GamepakBuilder,
    prelude::{GameBoyAdvance, NullAudio},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::info;

/// GBA button inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GbaButton {
    A,
    B,
    Start,
    Select,
    Up,
    Down,
    Left,
    Right,
    L,
    R,
}

impl GbaButton {
    /// Get all buttons.
    pub fn all() -> &'static [GbaButton] {
        &[
            GbaButton::A,
            GbaButton::B,
            GbaButton::Start,
            GbaButton::Select,
            GbaButton::Up,
            GbaButton::Down,
            GbaButton::Left,
            GbaButton::Right,
            GbaButton::L,
            GbaButton::R,
        ]
    }

    /// Convert to action string for LLM.
    pub fn to_action(&self) -> &'static str {
        match self {
            GbaButton::A => "press_a",
            GbaButton::B => "press_b",
            GbaButton::Start => "press_start",
            GbaButton::Select => "press_select",
            GbaButton::Up => "press_up",
            GbaButton::Down => "press_down",
            GbaButton::Left => "press_left",
            GbaButton::Right => "press_right",
            GbaButton::L => "press_l",
            GbaButton::R => "press_r",
        }
    }

    /// Parse from action string.
    pub fn from_action(action: &str) -> Option<Self> {
        match action {
            "press_a" | "a" => Some(GbaButton::A),
            "press_b" | "b" => Some(GbaButton::B),
            "press_start" | "start" => Some(GbaButton::Start),
            "press_select" | "select" => Some(GbaButton::Select),
            "press_up" | "up" => Some(GbaButton::Up),
            "press_down" | "down" => Some(GbaButton::Down),
            "press_left" | "left" => Some(GbaButton::Left),
            "press_right" | "right" => Some(GbaButton::Right),
            "press_l" | "l" => Some(GbaButton::L),
            "press_r" | "r" => Some(GbaButton::R),
            _ => None,
        }
    }

    /// Get the bit position for this button in the GBA keypad register.
    fn bit_position(&self) -> usize {
        match self {
            GbaButton::A => 0,
            GbaButton::B => 1,
            GbaButton::Select => 2,
            GbaButton::Start => 3,
            GbaButton::Right => 4,
            GbaButton::Left => 5,
            GbaButton::Up => 6,
            GbaButton::Down => 7,
            GbaButton::R => 8,
            GbaButton::L => 9,
        }
    }
}

/// Configuration for GBA emulator.
#[derive(Debug, Clone)]
pub struct GbaConfig {
    /// Path to the ROM file.
    pub rom_path: PathBuf,

    /// Path to BIOS file.
    pub bios_path: PathBuf,

    /// Enable audio output.
    pub audio_enabled: bool,

    /// Skip BIOS intro.
    pub skip_bios: bool,
}

/// GBA emulator with audio support.
pub struct GbaEmulator {
    gba: Box<GameBoyAdvance>,
    bios: Box<[u8]>,
    rom: Box<[u8]>,
    audio_player: Option<AudioPlayer>,
    frame_count: u64,
    pressed_buttons: HashSet<GbaButton>,
    screen_buffer: Vec<u32>,
}

impl GbaEmulator {
    /// Create a new GBA emulator.
    pub fn new(config: &GbaConfig) -> GbaResult<Self> {
        // Load ROM
        let rom_data: Box<[u8]> = std::fs::read(&config.rom_path)
            .map_err(|e| GbaError::RomLoad(e.to_string()))?
            .into_boxed_slice();

        // Build cartridge with save file support
        // Using file() instead of buffer() enables automatic .sav file persistence
        let cartridge = GamepakBuilder::new()
            .file(&config.rom_path)
            .build()
            .map_err(|e| GbaError::CartridgeBuild(format!("{:?}", e)))?;

        // Load BIOS
        let bios: Box<[u8]> = std::fs::read(&config.bios_path)
            .map_err(|e| GbaError::BiosLoad(e.to_string()))?
            .into_boxed_slice();

        // Create audio if enabled
        let (mut gba, audio_player) = if config.audio_enabled {
            let (audio_interface, player) = create_audio()?;
            let gba = GameBoyAdvance::new(bios.clone(), cartridge, audio_interface);
            info!("GBA emulator created with audio");
            (gba, Some(player))
        } else {
            let gba = GameBoyAdvance::new(bios.clone(), cartridge, NullAudio::new());
            info!("GBA emulator created without audio");
            (gba, None)
        };

        // Skip BIOS intro if requested
        if config.skip_bios {
            gba.skip_bios();
            info!("Skipping BIOS intro");
        }

        Ok(Self {
            gba: Box::new(gba),
            bios,
            rom: rom_data,
            audio_player,
            frame_count: 0,
            pressed_buttons: HashSet::new(),
            screen_buffer: vec![0; GBA_WIDTH * GBA_HEIGHT],
        })
    }

    /// Run a single frame of emulation.
    pub fn run_frame(&mut self) {
        self.gba.frame();
        self.frame_count += 1;

        // Update screen buffer
        let gpu_buffer = self.gba.get_frame_buffer();
        self.screen_buffer.copy_from_slice(gpu_buffer);
    }

    /// Sync keypad state to emulator.
    fn sync_keypad(&mut self) {
        let key_state = self.gba.get_key_state_mut();
        // All released = all bits set to 1
        *key_state = 0x03FF;
        // Pressed = bit set to 0
        for button in &self.pressed_buttons {
            key_state.set_bit(button.bit_position(), false);
        }
    }

    /// Press a button.
    pub fn press_button(&mut self, button: GbaButton) {
        self.pressed_buttons.insert(button);
        self.sync_keypad();
    }

    /// Release a button.
    pub fn release_button(&mut self, button: GbaButton) {
        self.pressed_buttons.remove(&button);
        self.sync_keypad();
    }

    /// Release all buttons.
    pub fn release_all(&mut self) {
        self.pressed_buttons.clear();
        let key_state = self.gba.get_key_state_mut();
        *key_state = 0x03FF;
    }

    /// Get current screen as RGBA pixels (u32 per pixel, 0x00RRGGBB format).
    pub fn screen_rgba(&self) -> &[u32] {
        &self.screen_buffer
    }

    /// Get current screen as RGB bytes.
    pub fn screen_rgb(&self) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(GBA_WIDTH * GBA_HEIGHT * 3);
        for pixel in &self.screen_buffer {
            rgb.push(((pixel >> 16) & 0xFF) as u8); // R
            rgb.push(((pixel >> 8) & 0xFF) as u8);  // G
            rgb.push((pixel & 0xFF) as u8);          // B
        }
        rgb
    }

    /// Get current screen as base64 PNG for LLM vision.
    pub fn screen_base64_png(&self) -> String {
        let mut png_data = Vec::new();
        {
            let mut encoder =
                png::Encoder::new(&mut png_data, GBA_WIDTH as u32, GBA_HEIGHT as u32);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);

            let mut writer = match encoder.write_header() {
                Ok(w) => w,
                Err(_) => return String::new(),
            };

            let rgb_data = self.screen_rgb();
            if writer.write_image_data(&rgb_data).is_err() {
                return String::new();
            }
        }

        BASE64.encode(&png_data)
    }

    /// Get frame count.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Create a save state.
    pub fn save_state(&self) -> GbaResult<Vec<u8>> {
        self.gba
            .save_state()
            .map_err(|e| GbaError::SaveState(format!("{:?}", e)))
    }

    /// Load a save state.
    pub fn load_state(&mut self, data: &[u8]) -> GbaResult<()> {
        // Need to recreate GBA with same audio setup
        let gba = if self.audio_player.is_some() {
            let (audio_interface, player) = create_audio()?;
            self.audio_player = Some(player);
            GameBoyAdvance::from_saved_state(data, self.bios.clone(), self.rom.clone(), audio_interface)
        } else {
            GameBoyAdvance::from_saved_state(data, self.bios.clone(), self.rom.clone(), NullAudio::new())
        }
        .map_err(|e| GbaError::SaveState(format!("{:?}", e)))?;

        self.gba = Box::new(gba);
        Ok(())
    }

    /// Check if audio is enabled.
    pub fn has_audio(&self) -> bool {
        self.audio_player.is_some()
    }
}
