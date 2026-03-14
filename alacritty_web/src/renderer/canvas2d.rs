//! Canvas 2D fallback renderer for the terminal.
//!
//! Draws terminal content using the browser's Canvas 2D API.
//! Works in every browser without WebGPU/WebGL requirements.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::NamedColor;

use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use super::colors;
use crate::terminal::WebEventProxy;

/// Font configuration for the Canvas 2D renderer.
#[derive(Debug, Clone)]
pub struct FontConfig {
    /// Font family CSS string (e.g. "'Fira Code', monospace").
    pub family: String,
    /// Font size in CSS pixels.
    pub size_px: f32,
    /// Line height multiplier (cell height = font_size * line_height_multiplier).
    pub line_height_multiplier: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "'Fira Code', 'Cascadia Code', 'Source Code Pro', monospace".to_string(),
            size_px: 14.0,
            line_height_multiplier: 1.4,
        }
    }
}

/// Canvas 2D-based terminal renderer.
pub struct Canvas2dRenderer {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    font_config: FontConfig,
    cell_width: f32,
    cell_height: f32,
    device_pixel_ratio: f64,
}

impl Canvas2dRenderer {
    /// Create a new Canvas 2D renderer with default font configuration.
    pub fn new(canvas: &HtmlCanvasElement) -> Result<Self, JsError> {
        let ctx = canvas
            .get_context("2d")
            .map_err(|e| JsError::new(&format!("Failed to get 2d context: {e:?}")))?
            .ok_or_else(|| JsError::new("No 2d context"))?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| JsError::new("Not a CanvasRenderingContext2d"))?;

        let font_config = FontConfig::default();

        // Measure cell dimensions.
        let font_str = format!("{}px {}", font_config.size_px, font_config.family);
        ctx.set_font(&font_str);
        let metrics = ctx
            .measure_text("M")
            .map_err(|e| JsError::new(&format!("measureText failed: {e:?}")))?;

        let cell_width = metrics.width() as f32;
        let cell_height = (font_config.size_px * font_config.line_height_multiplier).ceil();

        // Handle device pixel ratio for sharp text.
        let window = web_sys::window().ok_or_else(|| JsError::new("No window"))?;
        let dpr = window.device_pixel_ratio();

        // Set canvas backing store size.
        let css_width = canvas.client_width() as u32;
        let css_height = canvas.client_height() as u32;
        canvas.set_width((css_width as f64 * dpr) as u32);
        canvas.set_height((css_height as f64 * dpr) as u32);

        // Scale context for HiDPI.
        if let Err(e) = ctx.scale(dpr, dpr) {
            log::error!("Canvas scale failed: {e:?}");
        }

        log::info!(
            "Canvas2D renderer: cell={cell_width}x{cell_height}, dpr={dpr}, canvas={css_width}x{css_height}"
        );

        Ok(Self {
            canvas: canvas.clone(),
            ctx,
            font_config,
            cell_width,
            cell_height,
            device_pixel_ratio: dpr,
        })
    }

    /// Render the terminal state to the canvas.
    pub fn render(&self, term: &Term<WebEventProxy>) {
        let content = term.renderable_content();
        let term_colors = content.colors;
        let num_cols = term.columns();
        let num_lines = term.screen_lines();

        let bg_color = colors::default_named_color(NamedColor::Background);
        let _fg_default = colors::default_named_color(NamedColor::Foreground);

        let cw = self.cell_width as f64;
        let ch = self.cell_height as f64;
        let total_width = num_cols as f64 * cw;
        let total_height = num_lines as f64 * ch;

        // Update canvas backing store if needed.
        let dpr = self.device_pixel_ratio;
        let needed_w = (total_width * dpr).ceil() as u32;
        let needed_h = (total_height * dpr).ceil() as u32;
        if self.canvas.width() != needed_w || self.canvas.height() != needed_h {
            self.canvas.set_width(needed_w);
            self.canvas.set_height(needed_h);
            if let Err(e) = self.ctx.scale(dpr, dpr) {
                log::error!("Canvas scale failed during resize: {e:?}");
            }
        }

        // Clear with background color.
        self.ctx.set_fill_style_str(&format!(
            "rgb({},{},{})",
            bg_color.r, bg_color.g, bg_color.b
        ));
        self.ctx
            .fill_rect(0.0, 0.0, total_width + 10.0, total_height + 10.0);

        // Set font.
        let font_str = format!("{}px {}", self.font_config.size_px, self.font_config.family);
        self.ctx.set_font(&font_str);
        self.ctx.set_text_baseline("top");

        // Track which font variant we're using to minimize set_font calls.
        let mut current_bold = false;
        let mut current_italic = false;

        for indexed in content.display_iter {
            let point = indexed.point;
            let cell = &indexed.cell;

            // Skip wide char spacers.
            if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                continue;
            }

            let col = point.column.0 as f64;
            let row = point.line.0 as f64;

            let x = col * cw;
            let y = row * ch;

            // Resolve colors.
            let cell_fg = colors::resolve_color(&cell.fg, term_colors);
            let cell_bg = colors::resolve_color(&cell.bg, term_colors);

            // Draw background if not default.
            if cell_bg != bg_color {
                let width_mult = if cell.flags.contains(CellFlags::WIDE_CHAR) {
                    2.0
                } else {
                    1.0
                };
                self.ctx.set_fill_style_str(&format!(
                    "rgb({},{},{})",
                    cell_bg.r, cell_bg.g, cell_bg.b
                ));
                self.ctx.fill_rect(x, y, cw * width_mult, ch);
            }

            // Skip empty cells.
            if cell.c == ' ' || cell.c == '\t' || cell.c == '\0' {
                continue;
            }

            // Update font variant if needed.
            let is_bold = cell.flags.contains(CellFlags::BOLD);
            let is_italic = cell.flags.contains(CellFlags::ITALIC);
            if is_bold != current_bold || is_italic != current_italic {
                let weight = if is_bold { "bold " } else { "" };
                let style = if is_italic { "italic " } else { "" };
                let font_str = format!(
                    "{style}{weight}{}px {}",
                    self.font_config.size_px, self.font_config.family
                );
                self.ctx.set_font(&font_str);
                current_bold = is_bold;
                current_italic = is_italic;
            }

            // Draw character.
            self.ctx.set_fill_style_str(&format!(
                "rgb({},{},{})",
                cell_fg.r, cell_fg.g, cell_fg.b
            ));
            let ch_str = cell.c.to_string();
            if let Err(e) = self.ctx.fill_text(&ch_str, x, y + 2.0) {
                log::error!("fill_text failed: {e:?}");
            }
        }

        // Draw cursor.
        let cursor = &content.cursor;
        let cursor_color = colors::default_named_color(NamedColor::Cursor);
        let cx = cursor.point.column.0 as f64 * cw;
        let cy = cursor.point.line.0 as f64 * ch;
        self.ctx.set_fill_style_str(&format!(
            "rgba({},{},{},0.6)",
            cursor_color.r, cursor_color.g, cursor_color.b
        ));
        self.ctx.fill_rect(cx, cy, cw, ch);
    }

    /// Cell width in pixels.
    pub fn cell_width(&self) -> f32 {
        self.cell_width
    }

    /// Cell height in pixels.
    pub fn cell_height(&self) -> f32 {
        self.cell_height
    }

    /// Set the font size and remeasure cell dimensions.
    pub fn set_font_size(&mut self, size_px: f32) {
        self.font_config.size_px = size_px;
        self.remeasure_cells();
    }

    /// Set the font family and remeasure cell dimensions.
    pub fn set_font_family(&mut self, family: &str) {
        self.font_config.family = family.to_string();
        self.remeasure_cells();
    }

    /// Set the line height multiplier and remeasure cell dimensions.
    pub fn set_line_height_multiplier(&mut self, multiplier: f32) {
        self.font_config.line_height_multiplier = multiplier;
        self.remeasure_cells();
    }

    /// Remeasure cell dimensions after font config changes.
    fn remeasure_cells(&mut self) {
        let font_str = format!("{}px {}", self.font_config.size_px, self.font_config.family);
        self.ctx.set_font(&font_str);
        if let Ok(metrics) = self.ctx.measure_text("M") {
            self.cell_width = metrics.width() as f32;
            self.cell_height =
                (self.font_config.size_px * self.font_config.line_height_multiplier).ceil();
        }
    }

    /// Resize the canvas.
    pub fn resize(&self, _width: u32, _height: u32) {
        // Canvas resize is handled in render() based on terminal dimensions.
    }
}
