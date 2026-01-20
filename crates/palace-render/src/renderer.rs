//! Main renderer for Palace.

use crate::{DisplayProfile, DualScreen, FrameBuffer, RenderError, RenderResult, ScreenSide, TextRenderer, UISizing, UIState, ViewMode};
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

/// Configuration for the renderer.
#[derive(Debug, Clone)]
pub struct RenderConfig {
    /// Initial window width.
    pub width: u32,
    /// Initial window height.
    pub height: u32,
    /// Present mode (vsync control).
    pub present_mode: wgpu::PresentMode,
    /// Split ratio (left side).
    pub split_ratio: f32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            width: 3840,
            height: 2160,
            present_mode: wgpu::PresentMode::AutoVsync,
            split_ratio: 0.6,
        }
    }
}

/// Vertex for textured quads.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
    ];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// The main Palace renderer.
pub struct PalaceRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    game_texture: wgpu::Texture,
    game_bind_group: wgpu::BindGroup,
    dual_screen: DualScreen,
    ui_state: UIState,
    text_renderer: TextRenderer,
    ui_sizing: UISizing,
}

impl PalaceRenderer {
    /// Create a new renderer.
    pub async fn new(window: &Window, config: RenderConfig) -> RenderResult<Self> {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // SAFETY: The surface must not outlive the window.
        // In practice, the renderer is stored alongside the window.
        let surface = unsafe {
            instance.create_surface_unsafe(
                wgpu::SurfaceTargetUnsafe::from_window(window)
                    .map_err(|e| RenderError::Window(e.to_string()))?
            )
        }.map_err(|e| RenderError::Window(e.to_string()))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| RenderError::NoAdapter)?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                label: Some("palace_device"),
                memory_hints: Default::default(),
                trace: Default::default(),
                experimental_features: Default::default(),
            })
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: config.present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // Create shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/quad.wgsl").into()),
        });

        // Create texture for game screen (GBA: 240x160)
        let game_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("game_texture"),
            size: wgpu::Extent3d {
                width: 240,
                height: 160,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let game_texture_view = game_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let game_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest, // Pixel-perfect for GBA
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // Bind group layout and bind group
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let game_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("game_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&game_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&game_sampler),
                },
            ],
        });

        // Pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });

        // Render pipeline
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        // Quad vertices for left side (game)
        let vertices = Self::create_quad_vertices(config.split_ratio, ScreenSide::Left);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Create text renderer for UI
        let mut text_renderer = TextRenderer::new(&device, &queue, surface_format);
        text_renderer.resize(&queue, size.width, size.height);

        // Detect display profile (no controller yet - will be updated later)
        let profile = DisplayProfile::detect(false);
        let ui_sizing = UISizing::from_profile(profile);

        Ok(Self {
            surface,
            device,
            queue,
            config: surface_config,
            size,
            render_pipeline,
            vertex_buffer,
            index_buffer,
            game_texture,
            game_bind_group,
            dual_screen: DualScreen::new(config.split_ratio),
            ui_state: UIState::default(),
            text_renderer,
            ui_sizing,
        })
    }

    /// Create quad vertices for a screen side with given view mode.
    fn create_quad_vertices_for_mode(split_ratio: f32, side: ScreenSide, view_mode: ViewMode) -> [Vertex; 4] {
        let (left, right) = match view_mode {
            ViewMode::GameOnly | ViewMode::ConductorOnly => {
                // Full screen
                (-1.0, 1.0)
            }
            ViewMode::Split => match side {
                ScreenSide::Left => (-1.0, split_ratio * 2.0 - 1.0),
                ScreenSide::Right => (split_ratio * 2.0 - 1.0, 1.0),
            },
        };

        [
            Vertex { position: [left, 1.0], tex_coords: [0.0, 0.0] },
            Vertex { position: [left, -1.0], tex_coords: [0.0, 1.0] },
            Vertex { position: [right, -1.0], tex_coords: [1.0, 1.0] },
            Vertex { position: [right, 1.0], tex_coords: [1.0, 0.0] },
        ]
    }

    /// Create quad vertices for a screen side (split mode).
    fn create_quad_vertices(split_ratio: f32, side: ScreenSide) -> [Vertex; 4] {
        Self::create_quad_vertices_for_mode(split_ratio, side, ViewMode::Split)
    }

    /// Resize the renderer.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            self.text_renderer.resize(&self.queue, new_size.width, new_size.height);
        }
    }

    /// Update the game texture from a frame buffer.
    pub fn update_game_texture(&self, frame: &FrameBuffer) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.game_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * 4),
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Update UI state.
    pub fn update_ui(&mut self, state: UIState) {
        self.ui_state = state;
    }

    /// Update UI state (alias for update_ui).
    pub fn update_ui_state(&mut self, state: &UIState) {
        self.ui_state = state.clone();
    }

    /// Render a frame.
    pub fn render(&mut self) -> RenderResult<()> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Prepare UI text
        self.prepare_ui_text();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            // Render game on left side (or full screen depending on view mode)
            if self.dual_screen.show_game() {
                render_pass.set_pipeline(&self.render_pipeline);
                render_pass.set_bind_group(0, &self.game_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                render_pass.draw_indexed(0..6, 0, 0..1);
            }

            // Render text UI (confidence slider, status, etc.)
            let _ = self.text_renderer.render(
                &self.device,
                &self.queue,
                &mut render_pass,
            );
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        // Trim text atlas periodically
        self.text_renderer.trim();

        Ok(())
    }

    /// Prepare UI text for rendering.
    fn prepare_ui_text(&mut self) {
        self.text_renderer.clear();

        let width = self.size.width as f32;
        let height = self.size.height as f32;

        // Get sizes from display profile
        let font_size = self.ui_sizing.font_size;
        let header_size = self.ui_sizing.header_size;
        let small_size = self.ui_sizing.small_size;
        let padding = self.ui_sizing.padding;
        let line_height = self.ui_sizing.line_height;

        // Colors
        let white: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
        let dimmed: [f32; 4] = [0.5, 0.5, 0.5, 0.7];

        // Confidence slider (bottom-right)
        let slider_display = self.ui_state.slider.get_display();
        let char_width = font_size * 0.6;

        // Calculate positions from right edge
        let mut x = width - padding;

        // Next level (dimmed, rightmost)
        if let Some(next) = slider_display.next {
            let text_width = next.len() as f32 * char_width;
            x -= text_width;
            self.text_renderer.prepare(next, x, height - padding - font_size, dimmed, font_size);
            x -= padding;
        }

        // Current level (bright)
        let curr_width = slider_display.current.len() as f32 * char_width;
        x -= curr_width;
        self.text_renderer.prepare(slider_display.current, x, height - padding - font_size, white, font_size);
        x -= padding;

        // Previous level (dimmed, leftmost)
        if let Some(prev) = slider_display.previous {
            let text_width = prev.len() as f32 * char_width;
            x -= text_width;
            self.text_renderer.prepare(prev, x, height - padding - font_size, dimmed, font_size);
        }

        // Show conductor UI on right side when in split mode
        if self.dual_screen.show_conductor() {
            let conductor_x = match self.dual_screen.view_mode {
                ViewMode::Split => width * self.dual_screen.split_ratio + padding,
                ViewMode::ConductorOnly => padding,
                ViewMode::GameOnly => return, // No conductor visible
            };

            // Title
            self.text_renderer.prepare(
                "CONDUCTOR",
                conductor_x,
                padding,
                white,
                header_size,
            );

            // Status text size (2x small for readability)
            let status_size = font_size;

            // Focus indicator
            let focus_text = if self.dual_screen.game_focused() {
                "Focus: Game"
            } else {
                "Focus: Conductor"
            };
            let y_offset = header_size * line_height;
            self.text_renderer.prepare(focus_text, conductor_x, padding + y_offset, white, status_size);

            // Agent info
            let agent_text = format!(
                "Agent: {}/{}",
                self.ui_state.visible_agent + 1,
                self.ui_state.agent_count.max(1)
            );
            self.text_renderer.prepare(&agent_text, conductor_x, padding + y_offset + status_size * line_height, dimmed, status_size);

            // Display profile
            let profile_text = format!("Display: {}", self.ui_sizing.profile.name());
            self.text_renderer.prepare(&profile_text, conductor_x, padding + y_offset + status_size * line_height * 2.0, dimmed, status_size);

            // Status messages
            let mut y = padding + y_offset + status_size * line_height * 3.5;
            for msg in &self.ui_state.status_messages {
                let color = msg.level.color();
                self.text_renderer.prepare(&msg.text, conductor_x, y, color, status_size);
                y += status_size * line_height;
            }

            // LLM output (if any)
            if !self.ui_state.llm_output.is_empty() {
                y += padding;
                self.text_renderer.prepare("--- LLM Output ---", conductor_x, y, dimmed, status_size);
                y += status_size * line_height * 1.5;

                let llm_font_size = status_size * 0.85;
                for line in self.ui_state.llm_output.iter().rev().take(15) {
                    let color = line.output_type.color();
                    let prefix = format!("[{}] ", line.agent_name);
                    self.text_renderer.prepare(&prefix, conductor_x, y, dimmed, llm_font_size);
                    self.text_renderer.prepare(
                        &line.text,
                        conductor_x + prefix.len() as f32 * llm_font_size * 0.6,
                        y,
                        color,
                        llm_font_size,
                    );
                    y += llm_font_size * line_height;
                    if y > height - padding * 4.0 {
                        break;
                    }
                }
            }
        }
    }

    /// Get the device.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Get the queue.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Get current window size.
    pub fn size(&self) -> winit::dpi::PhysicalSize<u32> {
        self.size
    }

    /// Update the view mode and recalculate vertex positions.
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        if self.dual_screen.view_mode != mode {
            self.dual_screen.view_mode = mode;
            self.update_vertices();
        }
    }

    /// Update view mode and focus from DualScreen state.
    pub fn sync_dual_screen(&mut self, dual_screen: &DualScreen) {
        // Sync focus state
        self.dual_screen.focused = dual_screen.focused;

        // Sync view mode (requires vertex update)
        if self.dual_screen.view_mode != dual_screen.view_mode {
            self.dual_screen.view_mode = dual_screen.view_mode;
            self.update_vertices();
        }
    }

    /// Recalculate vertex buffer for current view mode.
    fn update_vertices(&mut self) {
        let vertices = Self::create_quad_vertices_for_mode(
            self.dual_screen.split_ratio,
            ScreenSide::Left,
            self.dual_screen.view_mode,
        );
        self.queue.write_buffer(
            &self.vertex_buffer,
            0,
            bytemuck::cast_slice(&vertices),
        );
    }

    /// Update display profile based on controller connection state.
    pub fn update_display_profile(&mut self, controller_connected: bool) {
        let new_profile = DisplayProfile::detect(controller_connected);
        if self.ui_sizing.profile != new_profile {
            self.ui_sizing.set_profile(new_profile);
            tracing::info!("Display profile changed to: {}", new_profile.name());
        }
    }

    /// Set a specific display profile (for manual override).
    pub fn set_display_profile(&mut self, profile: DisplayProfile) {
        self.ui_sizing.set_profile(profile);
        tracing::info!("Display profile set to: {}", profile.name());
    }

    /// Get current display profile.
    pub fn display_profile(&self) -> DisplayProfile {
        self.ui_sizing.profile
    }

    /// Get current UI sizing.
    pub fn ui_sizing(&self) -> &UISizing {
        &self.ui_sizing
    }
}
