//! Terminal renderers (wgpu and Canvas 2D fallback).

pub mod canvas2d;
pub mod colors;
#[cfg(feature = "wgpu")]
mod glyph_cache;
#[cfg(feature = "wgpu")]
mod rects;
#[cfg(feature = "wgpu")]
mod text;

#[cfg(feature = "wgpu")]
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::Term;
#[cfg(feature = "wgpu")]
use alacritty_terminal::vte::ansi::NamedColor;

use wasm_bindgen::prelude::*;
#[cfg(feature = "wgpu")]
use web_sys::HtmlCanvasElement;

use crate::terminal::WebEventProxy;

/// Trait abstracting terminal rendering backends.
pub trait TerminalRenderer {
    fn render(&mut self, term: &Term<WebEventProxy>);
    fn resize(&mut self, width: u32, height: u32);
    fn resize_backing_store(&mut self);
    fn cell_width(&self) -> f32;
    fn cell_height(&self) -> f32;
    fn set_font_size(&mut self, size_px: f32);
    fn set_font_family(&mut self, family: &str);
    fn set_line_height_multiplier(&mut self, multiplier: f32);
    fn set_focused(&mut self, focused: bool);
    fn backend_name(&self) -> &'static str;
}

/// The main wgpu renderer.
#[cfg(feature = "wgpu")]
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
    dpr: f64,
}

#[cfg(feature = "wgpu")]
impl WgpuRenderer {
    /// Initialize wgpu from a canvas element.
    pub async fn new(canvas: &HtmlCanvasElement) -> Result<Self, JsError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
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

        // Apply device pixel ratio for HiDPI.
        let dpr = web_sys::window().map(|w| w.device_pixel_ratio()).unwrap_or(1.0);
        let css_width = canvas.client_width().max(1) as u32;
        let css_height = canvas.client_height().max(1) as u32;
        let width = (css_width as f64 * dpr) as u32;
        let height = (css_height as f64 * dpr) as u32;

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
            "wgpu renderer initialized: {width}x{height} (dpr={dpr}), cell: {cell_width}x{cell_height}"
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
            dpr,
        })
    }

    /// Build instance data from terminal state and submit a GPU frame.
    fn render_frame(&mut self, term: &Term<WebEventProxy>) -> Result<(), JsError> {
        let content = term.renderable_content();
        let term_colors = content.colors;

        let mut text_instances = Vec::new();
        let mut rect_instances = Vec::new();

        let bg_color = colors::default_named_color(NamedColor::Background);

        for indexed in content.display_iter {
            let point = indexed.point;
            let cell = &indexed.cell;

            if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                continue;
            }

            let col = point.column.0 as f32;
            let row = point.line.0 as f32;

            let fg_rgb = colors::resolve_color(&cell.fg, term_colors);
            let bg_rgb = colors::resolve_color(&cell.bg, term_colors);

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

            if cell.c == ' ' || cell.c == '\t' || cell.c == '\0' {
                continue;
            }

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
            pos_y: cursor.point.line.0 as f32 * self.cell_height,
            size_w: self.cell_width,
            size_h: self.cell_height,
            color_r: cursor_color.r as f32 / 255.0,
            color_g: cursor_color.g as f32 / 255.0,
            color_b: cursor_color.b as f32 / 255.0,
            color_a: 0.5,
        });

        let vp_w = self.surface_config.width as f32;
        let vp_h = self.surface_config.height as f32;

        // Update GPU buffers.
        self.rect_renderer
            .update_instances(&self.device, &rect_instances);
        self.rect_renderer.update_projection(&self.queue, vp_w, vp_h);
        self.text_renderer.update_instances(
            &self.device,
            &self.queue,
            &text_instances,
            self.cell_width,
            self.cell_height,
            vp_w,
            vp_h,
            self.glyph_cache.as_ref(),
        );

        // Submit render pass.
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

            self.rect_renderer.draw(&mut render_pass);
            self.text_renderer.draw(&mut render_pass);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

#[cfg(feature = "wgpu")]
impl TerminalRenderer for WgpuRenderer {
    fn render(&mut self, term: &Term<WebEventProxy>) {
        if let Err(e) = self.render_frame(term) {
            log::warn!("wgpu render error: {e:?}");
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        let w = (width as f64 * self.dpr) as u32;
        let h = (height as f64 * self.dpr) as u32;
        if w > 0 && h > 0 {
            self.surface_config.width = w;
            self.surface_config.height = h;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }

    fn resize_backing_store(&mut self) {
        // wgpu backend resizes the surface via `resize` -- no separate backing store.
    }

    fn cell_width(&self) -> f32 {
        self.cell_width
    }

    fn cell_height(&self) -> f32 {
        self.cell_height
    }

    fn set_font_size(&mut self, _size_px: f32) {
        // TODO: rebuild glyph cache with new font size.
    }

    fn set_font_family(&mut self, _family: &str) {
        // TODO: rebuild glyph cache with new font family.
    }

    fn set_line_height_multiplier(&mut self, _multiplier: f32) {
        // TODO: rebuild glyph cache with new line height.
    }

    fn set_focused(&mut self, _focused: bool) {
        // TODO: mirror Canvas2D's hollow cursor shape in the wgpu backend.
    }

    fn backend_name(&self) -> &'static str {
        "wgpu"
    }
}
