//! Terminal renderers (wgpu and Canvas 2D fallback).

pub mod canvas2d;
pub mod colors;
mod glyph_cache;
mod rects;
mod text;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::NamedColor;

use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::terminal::WebEventProxy;

/// The main wgpu renderer.
pub struct WgpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    text_renderer: text::TextRenderer,
    rect_renderer: rects::RectRenderer,
    glyph_cache: Option<glyph_cache::GlyphCache>,
    cell_width: f32,
    cell_height: f32,
}

impl WgpuRenderer {
    /// Initialize wgpu from a canvas element.
    pub async fn new(canvas: &HtmlCanvasElement) -> Result<Self, JsError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            ..Default::default()
        });

        let surface_target = wgpu::SurfaceTarget::Canvas(canvas.clone());
        let surface = instance
            .create_surface(surface_target)
            .map_err(|e| JsError::new(&format!("Failed to create surface: {e}")))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| JsError::new("Failed to find a suitable GPU adapter"))?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("alacritty_device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|e| JsError::new(&format!("Failed to create device: {e}")))?;

        let width = canvas.client_width().max(1) as u32;
        let height = canvas.client_height().max(1) as u32;

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
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let text_renderer = text::TextRenderer::new(&device, surface_format);
        let rect_renderer = rects::RectRenderer::new(&device, surface_format);

        // Initialize glyph cache.
        let glyph_cache =
            glyph_cache::GlyphCache::new(&device, "'Fira Code', 'Cascadia Code', monospace", 14.0)
                .ok();

        let (cell_width, cell_height) = glyph_cache
            .as_ref()
            .map(|gc| (gc.cell_width(), gc.cell_height()))
            .unwrap_or((8.0, 16.0));

        log::info!(
            "wgpu renderer initialized: {width}x{height}, cell: {cell_width}x{cell_height}"
        );

        Ok(Self {
            device,
            queue,
            surface,
            surface_config,
            text_renderer,
            rect_renderer,
            glyph_cache,
            cell_width,
            cell_height,
        })
    }

    /// Resize the rendering surface.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }

    /// Update render state from terminal grid.
    pub fn update_from_terminal(&mut self, term: &Term<WebEventProxy>) {
        let content = term.renderable_content();
        let term_colors = content.colors;

        let mut text_instances = Vec::new();
        let mut rect_instances = Vec::new();

        let bg_color = colors::default_named_color(NamedColor::Background);

        for indexed in content.display_iter {
            let point = indexed.point;
            let cell = &indexed.cell;

            // Skip wide char spacers.
            if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                continue;
            }

            let col = point.column.0 as f32;
            let row = (point.line.0 as i32 + term.screen_lines() as i32) as f32;

            // Resolve colors.
            let fg_rgb = colors::resolve_color(&cell.fg, term_colors);
            let bg_rgb = colors::resolve_color(&cell.bg, term_colors);

            // Add background rect if not default.
            if bg_rgb != bg_color {
                let width_mult = if cell.flags.contains(CellFlags::WIDE_CHAR) {
                    2.0
                } else {
                    1.0
                };
                rect_instances.push(rects::RectInstance {
                    pos_x: col * self.cell_width,
                    pos_y: row * self.cell_height,
                    size_w: self.cell_width * width_mult,
                    size_h: self.cell_height,
                    color_r: bg_rgb.r as f32 / 255.0,
                    color_g: bg_rgb.g as f32 / 255.0,
                    color_b: bg_rgb.b as f32 / 255.0,
                    color_a: 1.0,
                });
            }

            // Skip empty/space cells for text rendering.
            if cell.c == ' ' || cell.c == '\t' || cell.c == '\0' {
                continue;
            }

            // Get glyph from cache.
            if let Some(cache) = &mut self.glyph_cache {
                let glyph_key = glyph_cache::GlyphKey {
                    character: cell.c,
                    bold: cell.flags.contains(CellFlags::BOLD),
                    italic: cell.flags.contains(CellFlags::ITALIC),
                };

                let glyph = cache.get_or_insert(&glyph_key, &self.queue);

                text_instances.push(text::CellInstance {
                    grid_col: col,
                    grid_row: row,
                    uv_x: glyph.uv_x,
                    uv_y: glyph.uv_y,
                    uv_w: glyph.uv_w,
                    uv_h: glyph.uv_h,
                    fg_r: fg_rgb.r as f32 / 255.0,
                    fg_g: fg_rgb.g as f32 / 255.0,
                    fg_b: fg_rgb.b as f32 / 255.0,
                    fg_a: 1.0,
                    bg_r: bg_rgb.r as f32 / 255.0,
                    bg_g: bg_rgb.g as f32 / 255.0,
                    bg_b: bg_rgb.b as f32 / 255.0,
                    bg_a: if bg_rgb != bg_color { 1.0 } else { 0.0 },
                });
            }
        }

        // Add cursor rect.
        let cursor = &content.cursor;
        let cursor_color = colors::default_named_color(NamedColor::Cursor);
        rect_instances.push(rects::RectInstance {
            pos_x: cursor.point.column.0 as f32 * self.cell_width,
            pos_y: (cursor.point.line.0 as i32 + term.screen_lines() as i32) as f32
                * self.cell_height,
            size_w: self.cell_width,
            size_h: self.cell_height,
            color_r: cursor_color.r as f32 / 255.0,
            color_g: cursor_color.g as f32 / 255.0,
            color_b: cursor_color.b as f32 / 255.0,
            color_a: 0.5,
        });

        // Update GPU buffers.
        self.rect_renderer
            .update_instances(&self.device, &rect_instances);
        self.text_renderer.update_instances(
            &self.device,
            &self.queue,
            &text_instances,
            self.cell_width,
            self.cell_height,
            self.surface_config.width as f32,
            self.surface_config.height as f32,
            self.glyph_cache.as_ref(),
        );
    }

    /// Render a frame.
    pub fn render(&mut self) -> Result<(), JsError> {
        let output = self
            .surface
            .get_current_texture()
            .map_err(|e| JsError::new(&format!("Surface error: {e}")))?;

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("render_encoder"),
                });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.114,
                            g: 0.122,
                            b: 0.129,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Draw rectangles (backgrounds, cursor, selection).
            self.rect_renderer.draw(&mut render_pass);

            // Draw text.
            self.text_renderer.draw(&mut render_pass);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Cell dimensions.
    pub fn cell_width(&self) -> f32 {
        self.cell_width
    }

    pub fn cell_height(&self) -> f32 {
        self.cell_height
    }
}
