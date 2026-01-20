//! Palace Render: wgpu-based rendering for the dual-screen UI.
//!
//! Provides GPU-accelerated rendering for:
//! - Dual-screen split view (Game on left, Orchestrator on right)
//! - Confidence slider in bottom-right corner
//! - LLM output streams with syntax highlighting
//! - Real-time code generation display

mod display;
mod renderer;
mod screen;
mod slider;
mod text;
mod ui;

pub use display::{DisplayProfile, UISizing};
pub use renderer::{PalaceRenderer, RenderConfig};
pub use screen::{DualScreen, ScreenSide, ViewMode};
pub use slider::ConfidenceSliderWidget;
pub use text::TextRenderer;
pub use ui::{OutputLine, OutputType, UIElement, UIState};

use wgpu::SurfaceError;

/// Error type for rendering operations.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("Surface error: {0}")]
    Surface(#[from] SurfaceError),

    #[error("No adapter found")]
    NoAdapter,

    #[error("Device request failed: {0}")]
    DeviceRequest(#[from] wgpu::RequestDeviceError),

    #[error("Window creation failed: {0}")]
    Window(String),

    #[error("Texture error: {0}")]
    Texture(String),

    #[error("Other: {0}")]
    Other(String),
}

pub type RenderResult<T> = Result<T, RenderError>;

/// Frame buffer for game/program rendering.
#[derive(Debug)]
pub struct FrameBuffer {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA pixel data.
    pub pixels: Vec<u8>,
}

impl FrameBuffer {
    /// Create a new frame buffer.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    /// Create from RGB data (converts to RGBA).
    pub fn from_rgb(width: u32, height: u32, rgb: &[u8]) -> Self {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for chunk in rgb.chunks(3) {
            pixels.push(chunk[0]); // R
            pixels.push(chunk[1]); // G
            pixels.push(chunk[2]); // B
            pixels.push(255);      // A
        }
        Self { width, height, pixels }
    }

    /// GBA screen dimensions (240x160).
    pub fn gba() -> Self {
        Self::new(240, 160)
    }

    /// Update pixel at position.
    pub fn set_pixel(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
        if x < self.width && y < self.height {
            let idx = ((y * self.width + x) * 4) as usize;
            self.pixels[idx] = r;
            self.pixels[idx + 1] = g;
            self.pixels[idx + 2] = b;
            self.pixels[idx + 3] = a;
        }
    }

    /// Clear to a color.
    pub fn clear(&mut self, r: u8, g: u8, b: u8, a: u8) {
        for chunk in self.pixels.chunks_exact_mut(4) {
            chunk[0] = r;
            chunk[1] = g;
            chunk[2] = b;
            chunk[3] = a;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_buffer() {
        let mut fb = FrameBuffer::new(10, 10);
        fb.set_pixel(5, 5, 255, 0, 0, 255);

        let idx = (5 * 10 + 5) * 4;
        assert_eq!(fb.pixels[idx], 255);
        assert_eq!(fb.pixels[idx + 1], 0);
        assert_eq!(fb.pixels[idx + 2], 0);
        assert_eq!(fb.pixels[idx + 3], 255);
    }

    #[test]
    fn test_gba_buffer() {
        let fb = FrameBuffer::gba();
        assert_eq!(fb.width, 240);
        assert_eq!(fb.height, 160);
    }
}
