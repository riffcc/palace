//! Text rendering using glyphon.

use glyphon::{
    Attrs, Buffer, Cache, Color, ColorMode, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonRenderer, Viewport,
};
use wgpu::{Device, MultisampleState, Queue, TextureFormat};

/// Text renderer for UI elements.
pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    cache: Cache,
    atlas: TextAtlas,
    viewport: Viewport,
    renderer: GlyphonRenderer,
    buffers: Vec<TextBuffer>,
    width: u32,
    height: u32,
}

/// A prepared text buffer ready for rendering.
pub struct TextBuffer {
    buffer: Buffer,
    x: f32,
    y: f32,
    color: Color,
    bounds_width: f32,
    bounds_height: f32,
}

impl TextRenderer {
    /// Create a new text renderer.
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let mut atlas = TextAtlas::with_color_mode(device, queue, &cache, format, ColorMode::Web);
        let viewport = Viewport::new(device, &cache);
        let renderer = GlyphonRenderer::new(&mut atlas, device, MultisampleState::default(), None);

        Self {
            font_system,
            swash_cache,
            cache,
            atlas,
            viewport,
            renderer,
            buffers: Vec::new(),
            width: 1,
            height: 1,
        }
    }

    /// Update viewport size.
    pub fn resize(&mut self, queue: &Queue, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.viewport.update(queue, Resolution { width, height });
    }

    /// Clear all prepared text.
    pub fn clear(&mut self) {
        self.buffers.clear();
    }

    /// Prepare text for rendering.
    pub fn prepare_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        font_size: f32,
        line_height: f32,
        color: [f32; 4],
        max_width: f32,
        max_height: f32,
    ) {
        let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(font_size, line_height));

        buffer.set_size(&mut self.font_system, Some(max_width), Some(max_height));

        buffer.set_text(
            &mut self.font_system,
            text,
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
            None, // No alignment override
        );

        buffer.shape_until_scroll(&mut self.font_system, false);

        let glyphon_color = Color::rgba(
            (color[0] * 255.0) as u8,
            (color[1] * 255.0) as u8,
            (color[2] * 255.0) as u8,
            (color[3] * 255.0) as u8,
        );

        self.buffers.push(TextBuffer {
            buffer,
            x,
            y,
            color: glyphon_color,
            bounds_width: max_width,
            bounds_height: max_height,
        });
    }

    /// Prepare text with default sizing.
    pub fn prepare(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        color: [f32; 4],
        font_size: f32,
    ) {
        self.prepare_text(
            text,
            x,
            y,
            font_size,
            font_size * 1.2,
            color,
            1000.0,
            100.0,
        );
    }

    /// Render all prepared text.
    pub fn render<'pass>(
        &'pass mut self,
        device: &Device,
        queue: &Queue,
        render_pass: &mut wgpu::RenderPass<'pass>,
    ) -> Result<(), String> {
        // Update viewport if size changed
        self.viewport
            .update(queue, Resolution {
                width: self.width,
                height: self.height,
            });

        // Build text areas from buffers
        let text_areas: Vec<TextArea> = self
            .buffers
            .iter()
            .map(|tb| TextArea {
                buffer: &tb.buffer,
                left: tb.x,
                top: tb.y,
                scale: 1.0,
                bounds: TextBounds {
                    left: tb.x as i32,
                    top: tb.y as i32,
                    right: (tb.x + tb.bounds_width) as i32,
                    bottom: (tb.y + tb.bounds_height) as i32,
                },
                default_color: tb.color,
                custom_glyphs: &[],
            })
            .collect();

        // Prepare glyphs
        self.renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .map_err(|e| format!("Text prepare error: {:?}", e))?;

        // Render
        self.renderer
            .render(&self.atlas, &self.viewport, render_pass)
            .map_err(|e| format!("Text render error: {:?}", e))?;

        Ok(())
    }

    /// Trim the atlas to free unused memory.
    pub fn trim(&mut self) {
        self.atlas.trim();
    }
}

#[cfg(test)]
mod tests {
    // Text renderer tests require GPU context, tested via examples
}
