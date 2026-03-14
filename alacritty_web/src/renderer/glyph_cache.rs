//! Glyph rasterization and atlas management using Canvas 2D API.
//!
//! Glyphs are rasterized via an offscreen canvas using `fillText()`,
//! then uploaded to a wgpu texture atlas.

use std::collections::HashMap;

use wasm_bindgen::prelude::*;

/// Key identifying a specific glyph in the cache.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct GlyphKey {
    pub character: char,
    pub bold: bool,
    pub italic: bool,
}

/// Cached glyph location in the atlas.
#[derive(Clone, Copy, Debug)]
pub struct GlyphEntry {
    /// UV coordinates in the atlas (normalized 0-1).
    pub uv_x: f32,
    pub uv_y: f32,
    pub uv_w: f32,
    pub uv_h: f32,
    /// Glyph metrics in pixels.
    pub width: u32,
    pub height: u32,
    pub bearing_x: f32,
    pub bearing_y: f32,
}

/// Atlas packer using simple row-based allocation.
struct AtlasPacker {
    width: u32,
    height: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
}

impl AtlasPacker {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
        }
    }

    /// Allocate space for a glyph. Returns (x, y) position in the atlas.
    fn allocate(&mut self, glyph_w: u32, glyph_h: u32) -> Option<(u32, u32)> {
        if self.cursor_x + glyph_w > self.width {
            // Move to next row.
            self.cursor_x = 0;
            self.cursor_y += self.row_height + 1;
            self.row_height = 0;
        }

        if self.cursor_y + glyph_h > self.height {
            return None; // Atlas full.
        }

        let pos = (self.cursor_x, self.cursor_y);
        self.cursor_x += glyph_w + 1;
        self.row_height = self.row_height.max(glyph_h);

        Some(pos)
    }
}

/// Glyph cache using Canvas 2D for rasterization and wgpu texture atlas.
pub struct GlyphCache {
    cache: HashMap<GlyphKey, GlyphEntry>,
    rasterize_canvas: web_sys::OffscreenCanvas,
    rasterize_ctx: web_sys::OffscreenCanvasRenderingContext2d,
    atlas_texture: wgpu::Texture,
    atlas_view: wgpu::TextureView,
    atlas_sampler: wgpu::Sampler,
    packer: AtlasPacker,
    font_family: String,
    font_size_px: f32,
    cell_width: f32,
    cell_height: f32,
}

const ATLAS_SIZE: u32 = 2048;

impl GlyphCache {
    /// Create a new glyph cache.
    pub fn new(
        device: &wgpu::Device,
        font_family: &str,
        font_size_px: f32,
    ) -> Result<Self, JsError> {
        // Create offscreen canvas for rasterization.
        let rasterize_canvas = web_sys::OffscreenCanvas::new(256, 256)
            .map_err(|e| JsError::new(&format!("Failed to create offscreen canvas: {e:?}")))?;

        let rasterize_ctx = rasterize_canvas
            .get_context("2d")
            .map_err(|e| JsError::new(&format!("Failed to get 2d context: {e:?}")))?
            .ok_or_else(|| JsError::new("No 2d context"))?
            .dyn_into::<web_sys::OffscreenCanvasRenderingContext2d>()
            .map_err(|_| JsError::new("Context is not OffscreenCanvasRenderingContext2d"))?;

        // Measure cell dimensions using the font.
        let font_str = format!("{font_size_px}px {font_family}");
        rasterize_ctx.set_font(&font_str);
        let metrics = rasterize_ctx.measure_text("M")
            .map_err(|e| JsError::new(&format!("measureText failed: {e:?}")))?;

        let cell_width = metrics.width() as f32;
        // Approximate cell height from font size (line height ~ 1.2x).
        let cell_height = (font_size_px * 1.2).ceil();

        // Create the atlas texture.
        let atlas_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let atlas_view = atlas_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph_atlas_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Ok(Self {
            cache: HashMap::new(),
            rasterize_canvas,
            rasterize_ctx,
            atlas_texture,
            atlas_view,
            atlas_sampler,
            packer: AtlasPacker::new(ATLAS_SIZE, ATLAS_SIZE),
            font_family: font_family.to_string(),
            font_size_px,
            cell_width,
            cell_height,
        })
    }

    /// Get or rasterize a glyph.
    pub fn get_or_insert(
        &mut self,
        key: &GlyphKey,
        queue: &wgpu::Queue,
    ) -> GlyphEntry {
        if let Some(entry) = self.cache.get(key) {
            return *entry;
        }

        let entry = self.rasterize_glyph(key, queue);
        self.cache.insert(key.clone(), entry);
        entry
    }

    /// Rasterize a glyph using Canvas 2D and upload to the atlas.
    fn rasterize_glyph(
        &mut self,
        key: &GlyphKey,
        queue: &wgpu::Queue,
    ) -> GlyphEntry {
        let ch = key.character.to_string();

        // Build font string.
        let weight = if key.bold { "bold " } else { "" };
        let style = if key.italic { "italic " } else { "" };
        let font_str = format!("{style}{weight}{}px {}", self.font_size_px, self.font_family);

        // Set up the rasterization canvas.
        let glyph_w = (self.cell_width.ceil() as u32).max(1);
        let glyph_h = (self.cell_height.ceil() as u32).max(1);

        self.rasterize_canvas.set_width(glyph_w);
        self.rasterize_canvas.set_height(glyph_h);

        let ctx = &self.rasterize_ctx;
        ctx.set_font(&font_str);
        ctx.set_fill_style_str("white");
        ctx.set_text_baseline("top");

        // Clear and draw.
        ctx.clear_rect(0.0, 0.0, glyph_w as f64, glyph_h as f64);
        if let Err(e) = ctx.fill_text(&ch, 0.0, 0.0) {
            log::error!("Glyph cache fill_text failed: {e:?}");
        }

        // Read pixel data.
        let image_data = ctx
            .get_image_data(0.0, 0.0, glyph_w as f64, glyph_h as f64)
            .ok();

        // Allocate space in the atlas.
        let (atlas_x, atlas_y) = self.packer.allocate(glyph_w, glyph_h).unwrap_or((0, 0));

        // Extract alpha channel and upload to atlas.
        if let Some(image_data) = image_data {
            let rgba = image_data.data();
            let mut alpha_data = vec![0u8; (glyph_w * glyph_h) as usize];
            for i in 0..alpha_data.len() {
                // Use red channel (white text on transparent bg).
                alpha_data[i] = rgba[i * 4];
            }

            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: atlas_x,
                        y: atlas_y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &alpha_data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(glyph_w),
                    rows_per_image: Some(glyph_h),
                },
                wgpu::Extent3d {
                    width: glyph_w,
                    height: glyph_h,
                    depth_or_array_layers: 1,
                },
            );
        }

        let atlas_size = ATLAS_SIZE as f32;
        GlyphEntry {
            uv_x: atlas_x as f32 / atlas_size,
            uv_y: atlas_y as f32 / atlas_size,
            uv_w: glyph_w as f32 / atlas_size,
            uv_h: glyph_h as f32 / atlas_size,
            width: glyph_w,
            height: glyph_h,
            bearing_x: 0.0,
            bearing_y: 0.0,
        }
    }

    /// Get the atlas texture view.
    pub fn atlas_view(&self) -> &wgpu::TextureView {
        &self.atlas_view
    }

    /// Get the atlas sampler.
    pub fn atlas_sampler(&self) -> &wgpu::Sampler {
        &self.atlas_sampler
    }

    /// Cell dimensions in pixels.
    pub fn cell_width(&self) -> f32 {
        self.cell_width
    }

    pub fn cell_height(&self) -> f32 {
        self.cell_height
    }
}
