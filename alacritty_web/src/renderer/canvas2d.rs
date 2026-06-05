//! Canvas 2D fallback renderer for the terminal.
//!
//! Draws terminal content using the browser's Canvas 2D API.
//! Works in every browser without WebGPU/WebGL requirements.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Rgb};

use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use super::colors;
use super::TerminalRenderer;
use crate::terminal::WebEventProxy;

/// Force Canvas 2D to use uniform glyph advance: disable pair kerning and all
/// OpenType ligature/contextual-alternates features, and clamp letter spacing.
/// Properties are not on the web-sys typed binding, so we set them via Reflect.
fn disable_kerning(ctx: &CanvasRenderingContext2d) {
    let obj: &JsValue = ctx.as_ref();
    let _ = js_sys::Reflect::set(
        obj,
        &JsValue::from_str("fontKerning"),
        &JsValue::from_str("none"),
    );
    let _ = js_sys::Reflect::set(
        obj,
        &JsValue::from_str("letterSpacing"),
        &JsValue::from_str("0px"),
    );
    // Chrome-only: disable discretionary font features like ligatures.
    let _ = js_sys::Reflect::set(
        obj,
        &JsValue::from_str("fontVariantCaps"),
        &JsValue::from_str("normal"),
    );
    let _ = js_sys::Reflect::set(
        obj,
        &JsValue::from_str("textRendering"),
        &JsValue::from_str("geometricPrecision"),
    );
}

/// Font configuration for the terminal renderer.
#[derive(Clone, Debug)]
pub struct FontConfig {
    pub family: String,
    pub size_px: f32,
    pub line_height_multiplier: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            // Default to fonts without contextual ligatures so cell advance is uniform.
            family: "ui-monospace, 'SF Mono', Menlo, Consolas, 'DejaVu Sans Mono', monospace"
                .to_string(),
            size_px: 14.0,
            line_height_multiplier: 1.2,
        }
    }
}

/// How (if at all) a cell's underline decoration should be drawn. Native
/// alacritty treats these as mutually exclusive — the last-set SGR sub-style
/// wins, so we collapse the flag set into a single discriminant.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UnderlineStyle {
    None,
    Solid,
    Double,
    Dotted,
    Dashed,
    Curly,
}

/// One cell's renderable state after style resolution + selection inversion.
#[derive(Clone, Copy)]
struct CellView {
    ch: char,
    fg: Rgb,
    bg: Rgb,
    bold: bool,
    italic: bool,
    wide: bool,
    underline: UnderlineStyle,
    strikeout: bool,
}

/// Pack an Rgb into a u32 key for the colour cache. Top byte unused.
#[inline]
fn rgb_key(c: Rgb) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | (c.b as u32)
}

/// Canvas 2D-based terminal renderer.
pub struct Canvas2dRenderer {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    font_family: String,
    font_size_px: f32,
    line_height_multiplier: f32,
    cell_width: f32,
    cell_height: f32,
    device_pixel_ratio: f64,
    /// Whether the host canvas has keyboard focus; controls solid vs hollow cursor.
    focused: bool,
    /// Bell-flash intensity in [0,1]. Drawn as a translucent white overlay
    /// after the main paint. Zero = no overlay.
    bell_intensity: f32,
    /// Persistent "rgb(r,g,b)" string cache. Each lookup saves a `format!` +
    /// the wasm→JS UTF-16 conversion of a freshly-allocated string. For
    /// typical workloads the working set is the 16/256-color palette plus a
    /// handful of truecolor values, so the map stays small.
    color_cache: RefCell<HashMap<u32, String>>,
    /// Most recently applied fill style. `set_fill_style_str` is cheap-ish but
    /// every call still costs a wasm→JS hop and CSS string parse; tracking the
    /// current colour eliminates the redundant ones across passes / rows.
    current_fill: Cell<Option<u32>>,
    /// Reusable per-frame grid buffer. Sized to `screen_lines × columns` once
    /// and cleared between frames so we don't re-allocate ~60KB every paint.
    grid_buf: RefCell<Vec<Vec<Option<CellView>>>>,
    /// Reusable scratch String for text runs in pass 2 — `String::new` per
    /// run is otherwise the biggest allocator in the renderer.
    text_run: RefCell<String>,
}

impl Canvas2dRenderer {
    /// Create a new Canvas 2D renderer.
    pub fn new(canvas: &HtmlCanvasElement) -> Result<Self, JsError> {
        let ctx = canvas
            .get_context("2d")
            .map_err(|e| JsError::new(&format!("Failed to get 2d context: {e:?}")))?
            .ok_or_else(|| JsError::new("No 2d context"))?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| JsError::new("Not a CanvasRenderingContext2d"))?;

        let default_font = FontConfig::default();
        let font_family = default_font.family.clone();
        let font_size_px = default_font.size_px;
        let line_height_multiplier = default_font.line_height_multiplier;

        // Disable kerning/ligatures so each cell advances by a constant width.
        disable_kerning(&ctx);

        // Measure cell advance over a long run; averaging avoids fractional jitter
        // on a single glyph.
        let font_str = format!("{font_size_px}px {font_family}");
        ctx.set_font(&font_str);
        let probe: String = "M".repeat(80);
        let metrics = ctx
            .measure_text(&probe)
            .map_err(|e| JsError::new(&format!("measureText failed: {e:?}")))?;
        // Round down to the nearest pixel so chars never overflow their cell.
        let cell_width = (metrics.width() as f32 / 80.0).floor().max(1.0);
        let cell_height = (font_size_px * line_height_multiplier).ceil();

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
            log::warn!("Canvas scale failed: {e:?}");
        }

        log::info!(
            "Canvas2D renderer: cell={cell_width}x{cell_height}, dpr={dpr}, canvas={css_width}x{css_height}"
        );

        Ok(Self {
            canvas: canvas.clone(),
            ctx,
            font_family,
            font_size_px,
            line_height_multiplier,
            cell_width,
            cell_height,
            device_pixel_ratio: dpr,
            focused: true,
            bell_intensity: 0.0,
            color_cache: RefCell::new(HashMap::with_capacity(64)),
            current_fill: Cell::new(None),
            grid_buf: RefCell::new(Vec::new()),
            text_run: RefCell::new(String::with_capacity(64)),
        })
    }

    /// Look up or insert an `"rgb(r,g,b)"` string for the given colour, then
    /// apply it as the fill style — but only if it differs from the colour
    /// already set on the context. Returns true if the style actually
    /// changed (caller doesn't currently need this, but it's free).
    fn apply_fill(&self, rgb: Rgb) -> bool {
        let key = rgb_key(rgb);
        if self.current_fill.get() == Some(key) {
            return false;
        }
        let mut cache = self.color_cache.borrow_mut();
        let css = cache
            .entry(key)
            .or_insert_with(|| format!("rgb({},{},{})", rgb.r, rgb.g, rgb.b));
        self.ctx.set_fill_style_str(css);
        self.current_fill.set(Some(key));
        true
    }

    /// Same as `apply_fill` but for the stroke style. Doesn't share the
    /// `current_fill` tracking because strokes are rare (only the unfocused
    /// cursor) — separate cache miss is cheap.
    fn apply_stroke(&self, rgb: Rgb) {
        let key = rgb_key(rgb);
        let mut cache = self.color_cache.borrow_mut();
        let css = cache
            .entry(key)
            .or_insert_with(|| format!("rgb({},{},{})", rgb.r, rgb.g, rgb.b));
        self.ctx.set_stroke_style_str(css);
    }

    /// Draw one underline run with the primitive appropriate for its style.
    /// `y_base` is the top of the cell row; `ch` is the cell height. All
    /// styles draw inside the bottom strip of the cell so they don't collide
    /// with descenders.
    fn flush_underline(
        &self,
        run: &mut Option<(usize, Rgb, UnderlineStyle, f64)>,
        y_base: f64,
        ch: f64,
        line_thick: f64,
        cell_w: f64,
    ) {
        let Some((start_col, color, style, width)) = run.take() else { return };
        let x0 = start_col as f64 * cell_w;
        // Primary underline sits 1 px above the cell's bottom edge.
        let y_solid = y_base + ch - line_thick - 1.0;
        match style {
            UnderlineStyle::None => {}
            UnderlineStyle::Solid => {
                self.apply_fill(color);
                self.ctx.fill_rect(x0, y_solid, width, line_thick);
            }
            UnderlineStyle::Double => {
                // Two parallel bars within the bottom strip — separated by a
                // gap >= line_thick so they read as two.
                let gap = line_thick.max(1.0);
                let y_upper = y_solid - line_thick - gap;
                self.apply_fill(color);
                self.ctx.fill_rect(x0, y_upper, width, line_thick);
                self.ctx.fill_rect(x0, y_solid, width, line_thick);
            }
            UnderlineStyle::Dotted => {
                self.apply_stroke(color);
                self.ctx.set_line_width(line_thick);
                let dash = js_sys::Array::new();
                dash.push(&JsValue::from(line_thick));
                dash.push(&JsValue::from(line_thick));
                let _ = self.ctx.set_line_dash(&dash);
                self.ctx.begin_path();
                let y = y_solid + line_thick / 2.0;
                self.ctx.move_to(x0, y);
                self.ctx.line_to(x0 + width, y);
                self.ctx.stroke();
                let _ = self.ctx.set_line_dash(&js_sys::Array::new());
            }
            UnderlineStyle::Dashed => {
                self.apply_stroke(color);
                self.ctx.set_line_width(line_thick);
                let dash = js_sys::Array::new();
                dash.push(&JsValue::from(line_thick * 3.0));
                dash.push(&JsValue::from(line_thick * 2.0));
                let _ = self.ctx.set_line_dash(&dash);
                self.ctx.begin_path();
                let y = y_solid + line_thick / 2.0;
                self.ctx.move_to(x0, y);
                self.ctx.line_to(x0 + width, y);
                self.ctx.stroke();
                let _ = self.ctx.set_line_dash(&js_sys::Array::new());
            }
            UnderlineStyle::Curly => {
                // Zig-zag of quadratic half-arcs alternating above and below
                // a centre line. Amplitude ~line_thick so the wave is
                // visually distinct from a solid bar without overflowing
                // into the next cell's descender region.
                self.apply_stroke(color);
                self.ctx.set_line_width(line_thick);
                let period = (cell_w * 0.6).max(4.0);
                let amp = (line_thick * 1.5).max(2.0);
                // Centre line raised slightly above the solid position so
                // the wave's lower peak still lands inside the cell.
                let y_mid = y_solid - amp / 2.0;
                self.ctx.begin_path();
                self.ctx.move_to(x0, y_mid);
                let mut up = true;
                let mut x = x0;
                let end = x0 + width;
                while x < end {
                    let nx = (x + period).min(end);
                    let cx = (x + nx) / 2.0;
                    let cy = if up { y_mid - amp } else { y_mid + amp };
                    self.ctx.quadratic_curve_to(cx, cy, nx, y_mid);
                    up = !up;
                    x = nx;
                }
                self.ctx.stroke();
            }
        }
    }

    /// Render the terminal state to the canvas.
    pub fn render(&self, term: &Term<WebEventProxy>) {
        let content = term.renderable_content();
        let term_colors = content.colors;
        // `point.line.0` is measured from the bottom of the screen and can be
        // negative when the user scrolls back into history. Adding
        // `display_offset` normalises it back into a `0..screen_lines` row
        // index inside the viewport.
        let display_offset = content.display_offset as i32;

        let bg_color = colors::default_named_color(NamedColor::Background);

        let cw = self.cell_width as f64;
        let ch = self.cell_height as f64;
        // Vertical offset so the ascent of the glyph sits inside the cell instead of at the top.
        let text_y_offset = ((ch - self.font_size_px as f64) / 2.0).max(0.0);

        // Work in CSS pixels. The backing-store resize is handled externally
        // by `resize_backing_store`, triggered from the JS-side ResizeObserver,
        // because touching canvas.width here causes a synchronous reflow that
        // can re-enter WASM from ResizeObserver and crash.
        let canvas_width = self.canvas.client_width().max(1) as f64;
        let canvas_height = self.canvas.client_height().max(1) as f64;

        // Invalidate the fill-style cache for this frame; the context still
        // remembers the previous frame's last colour, but it's cheaper to
        // re-apply once than to track context state across frames.
        self.current_fill.set(None);

        // Clear entire canvas with background color.
        self.apply_fill(bg_color);
        self.ctx.fill_rect(0.0, 0.0, canvas_width, canvas_height);

        // Set initial font + state. disable_kerning must be re-applied after
        // every set_width/set_height because those operations reset the context.
        disable_kerning(&self.ctx);
        let font_str = format!("{}px {}", self.font_size_px, self.font_family);
        self.ctx.set_font(&font_str);
        self.ctx.set_text_baseline("top");

        let screen_lines = term.screen_lines();
        let columns = term.columns();
        // Re-use the grid buffer across frames; only re-shape it when the
        // grid dimensions actually change (rare). Otherwise we just blank
        // every cell to None.
        let mut grid = self.grid_buf.borrow_mut();
        if grid.len() != screen_lines || grid.first().map(|r| r.len()) != Some(columns) {
            grid.clear();
            grid.resize_with(screen_lines, || vec![None; columns]);
        } else {
            for row in grid.iter_mut() {
                for cell in row.iter_mut() {
                    *cell = None;
                }
            }
        }

        let selection_range = content.selection;

        // Pre-resolve the default Indexed (cell.bg == Color::Named(Background))
        // pair so we can early-skip cells that contribute nothing to render —
        // i.e. blank background AND blank/space char AND no decorations. In a
        // typical 80x30 viewport with output spread across half the rows,
        // that's ~half of `display_iter`'s yields.
        const RENDERABLE_FLAGS: CellFlags = CellFlags::UNDERLINE
            .union(CellFlags::DOUBLE_UNDERLINE)
            .union(CellFlags::DOTTED_UNDERLINE)
            .union(CellFlags::DASHED_UNDERLINE)
            .union(CellFlags::UNDERCURL)
            .union(CellFlags::STRIKEOUT);

        for indexed in content.display_iter {
            let point = indexed.point;
            let cell = &indexed.cell;
            if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                continue;
            }
            let row = point.line.0 + display_offset;
            let col = point.column.0;
            if row < 0 || (row as usize) >= screen_lines || col >= columns {
                continue;
            }
            // Hot fast-path: skip the colour resolve + CellView build for
            // cells that are visually identical to the cleared background.
            let blank_char = matches!(cell.c, ' ' | '\0' | '\t');
            let default_bg = matches!(cell.bg, AnsiColor::Named(NamedColor::Background));
            if blank_char && default_bg && !cell.flags.intersects(RENDERABLE_FLAGS) {
                continue;
            }
            let mut fg = colors::resolve_color(&cell.fg, term_colors);
            let mut bg = colors::resolve_color(&cell.bg, term_colors);
            // Dim: lerp the foreground halfway toward the background.
            if cell.flags.contains(CellFlags::DIM) || cell.flags.contains(CellFlags::DIM_BOLD) {
                fg = Rgb {
                    r: ((fg.r as u16 + bg.r as u16) / 2) as u8,
                    g: ((fg.g as u16 + bg.g as u16) / 2) as u8,
                    b: ((fg.b as u16 + bg.b as u16) / 2) as u8,
                };
            }
            // Hidden cells render as if the glyph were a space.
            let glyph = if cell.flags.contains(CellFlags::HIDDEN) { ' ' } else { cell.c };
            // Invert fg/bg for selected cells so the user can see what they've
            // highlighted.
            if let Some(range) = selection_range {
                if range.contains(point) {
                    std::mem::swap(&mut fg, &mut bg);
                }
            }
            // Priority order matches native alacritty's SGR handling: a
            // CURLY beats DOUBLE beats DASHED beats DOTTED beats plain. OSC 8
            // hyperlinks fall through to a solid underline so the link is
            // visible at a glance — a JS hover cursor change is on top of this.
            let underline = if cell.flags.contains(CellFlags::UNDERCURL) {
                UnderlineStyle::Curly
            } else if cell.flags.contains(CellFlags::DOUBLE_UNDERLINE) {
                UnderlineStyle::Double
            } else if cell.flags.contains(CellFlags::DASHED_UNDERLINE) {
                UnderlineStyle::Dashed
            } else if cell.flags.contains(CellFlags::DOTTED_UNDERLINE) {
                UnderlineStyle::Dotted
            } else if cell.flags.contains(CellFlags::UNDERLINE)
                || cell.hyperlink().is_some()
            {
                UnderlineStyle::Solid
            } else {
                UnderlineStyle::None
            };
            let view = CellView {
                ch: glyph,
                fg,
                bg,
                bold: cell.flags.contains(CellFlags::BOLD)
                    || cell.flags.contains(CellFlags::DIM_BOLD),
                italic: cell.flags.contains(CellFlags::ITALIC),
                wide: cell.flags.contains(CellFlags::WIDE_CHAR),
                underline,
                strikeout: cell.flags.contains(CellFlags::STRIKEOUT),
            };
            grid[row as usize][col] = Some(view);
        }

        // Pass 1: draw non-default backgrounds, coalescing consecutive cells.
        for (row_idx, row) in grid.iter().enumerate() {
            let y = row_idx as f64 * ch;
            let mut run_start: Option<(usize, Rgb, f64)> = None;
            for (col_idx, cell) in row.iter().enumerate() {
                let Some(cv) = cell else {
                    if let Some((start, bg, w)) = run_start.take() {
                        self.apply_fill(bg);
                        self.ctx.fill_rect(start as f64 * cw, y, w, ch);
                    }
                    continue;
                };
                if cv.bg == bg_color {
                    if let Some((start, bg, w)) = run_start.take() {
                        self.apply_fill(bg);
                        self.ctx.fill_rect(start as f64 * cw, y, w, ch);
                    }
                    continue;
                }
                let cell_w = if cv.wide { cw * 2.0 } else { cw };
                match &mut run_start {
                    Some((_, existing_bg, w)) if *existing_bg == cv.bg => {
                        *w += cell_w;
                    }
                    _ => {
                        if let Some((start, bg, w)) = run_start.take() {
                            self.apply_fill(bg);
                            self.ctx.fill_rect(start as f64 * cw, y, w, ch);
                        }
                        run_start = Some((col_idx, cv.bg, cell_w));
                    }
                }
            }
            if let Some((start, bg, w)) = run_start.take() {
                self.apply_fill(bg);
                self.ctx.fill_rect(start as f64 * cw, y, w, ch);
            }
        }

        // Pass 2: draw text runs, coalescing consecutive cells that share
        // foreground color and font style. Each run is rendered with a single
        // fillText. The run text is kept in a reusable String (`self.text_run`)
        // so we don't allocate a fresh `String` for every run.
        let mut current_bold = false;
        let mut current_italic = false;
        let mut text_run = self.text_run.borrow_mut();
        // run = (start_col, fg, bold, italic). text accumulates into `text_run`.
        let mut run: Option<(usize, Rgb, bool, bool)> = None;
        for (row_idx, row) in grid.iter().enumerate() {
            let y = row_idx as f64 * ch;
            // `run` is always None at this point — the previous row's
            // end-of-row flush below clears it. Asserted nowhere but it's a
            // tight enough invariant to keep visible in the code shape.
            for (col_idx, cell) in row.iter().enumerate() {
                let Some(cv) = cell else {
                    if let Some((start, fg, bold, italic)) = run.take() {
                        if current_bold != bold || current_italic != italic {
                            let weight = if bold { "bold " } else { "" };
                            let style = if italic { "italic " } else { "" };
                            let font_str = format!(
                                "{style}{weight}{}px {}",
                                self.font_size_px, self.font_family
                            );
                            self.ctx.set_font(&font_str);
                            current_bold = bold;
                            current_italic = italic;
                        }
                        self.apply_fill(fg);
                        let _ = self.ctx.fill_text(
                            &text_run,
                            start as f64 * cw,
                            y + text_y_offset,
                        );
                        text_run.clear();
                    }
                    continue;
                };
                if cv.ch == ' ' || cv.ch == '\0' || cv.ch == '\t' {
                    if let Some((start, fg, bold, italic)) = run.take() {
                        if current_bold != bold || current_italic != italic {
                            let weight = if bold { "bold " } else { "" };
                            let style = if italic { "italic " } else { "" };
                            let font_str = format!(
                                "{style}{weight}{}px {}",
                                self.font_size_px, self.font_family
                            );
                            self.ctx.set_font(&font_str);
                            current_bold = bold;
                            current_italic = italic;
                        }
                        self.apply_fill(fg);
                        let _ = self.ctx.fill_text(
                            &text_run,
                            start as f64 * cw,
                            y + text_y_offset,
                        );
                        text_run.clear();
                    }
                    continue;
                }
                match &mut run {
                    Some((_, fg, bold, italic))
                        if *fg == cv.fg && *bold == cv.bold && *italic == cv.italic =>
                    {
                        text_run.push(cv.ch);
                    }
                    _ => {
                        if let Some((start, fg, bold, italic)) = run.take() {
                            if current_bold != bold || current_italic != italic {
                                let weight = if bold { "bold " } else { "" };
                                let style = if italic { "italic " } else { "" };
                                let font_str = format!(
                                    "{style}{weight}{}px {}",
                                    self.font_size_px, self.font_family
                                );
                                self.ctx.set_font(&font_str);
                                current_bold = bold;
                                current_italic = italic;
                            }
                            self.apply_fill(fg);
                            let _ = self.ctx.fill_text(
                                &text_run,
                                start as f64 * cw,
                                y + text_y_offset,
                            );
                            text_run.clear();
                        }
                        run = Some((col_idx, cv.fg, cv.bold, cv.italic));
                        text_run.push(cv.ch);
                    }
                }
            }
            // End-of-row flush.
            if let Some((start, fg, bold, italic)) = run.take() {
                if current_bold != bold || current_italic != italic {
                    let weight = if bold { "bold " } else { "" };
                    let style = if italic { "italic " } else { "" };
                    let font_str =
                        format!("{style}{weight}{}px {}", self.font_size_px, self.font_family);
                    self.ctx.set_font(&font_str);
                    current_bold = bold;
                    current_italic = italic;
                }
                self.apply_fill(fg);
                let _ = self
                    .ctx
                    .fill_text(&text_run, start as f64 * cw, y + text_y_offset);
                text_run.clear();
            }
        }
        drop(text_run);

        // Pass 3: line decorations. Drawn after the text so they overlay
        // glyphs. We coalesce consecutive cells that share (style, fg) into
        // a single run, then draw the run with the right primitive:
        //   Solid / Double → fill_rect (cheapest)
        //   Dotted / Dashed → stroke with setLineDash
        //   Curly           → stroke with a quadratic-curve zig-zag
        let strikeout_y = (ch / 2.0).floor();
        let line_thick = (ch / 14.0).max(1.0).round();

        // Underline runs: coalesce by (style, colour).
        for (row_idx, row) in grid.iter().enumerate() {
            let y_base = row_idx as f64 * ch;
            let mut run: Option<(usize, Rgb, UnderlineStyle, f64)> = None;
            for (col_idx, cell) in row.iter().enumerate() {
                let cur = match cell {
                    Some(cv) if cv.underline != UnderlineStyle::None => Some((cv.fg, cv.underline)),
                    _ => None,
                };
                let cell_w = match cell {
                    Some(cv) if cv.wide => cw * 2.0,
                    _ => cw,
                };
                match (&mut run, cur) {
                    (Some((_, c, s, w)), Some((fg, style))) if *c == fg && *s == style => {
                        *w += cell_w;
                    }
                    (_, Some((fg, style))) => {
                        self.flush_underline(&mut run, y_base, ch, line_thick, cw);
                        run = Some((col_idx, fg, style, cell_w));
                    }
                    (_, None) => {
                        self.flush_underline(&mut run, y_base, ch, line_thick, cw);
                    }
                }
            }
            self.flush_underline(&mut run, y_base, ch, line_thick, cw);
        }

        // Strikeout pass: orthogonal to underline style, so kept separate.
        for (row_idx, row) in grid.iter().enumerate() {
            let y = row_idx as f64 * ch + strikeout_y;
            let mut run: Option<(usize, Rgb, f64)> = None;
            for (col_idx, cell) in row.iter().enumerate() {
                let cell_w = match cell {
                    Some(cv) if cv.wide => cw * 2.0,
                    _ => cw,
                };
                let strike_fg = match cell {
                    Some(cv) if cv.strikeout => Some(cv.fg),
                    _ => None,
                };
                match (&mut run, strike_fg) {
                    (Some((_, c, w)), Some(fg)) if *c == fg => *w += cell_w,
                    (_, Some(fg)) => {
                        if let Some((s, c, w)) = run.take() {
                            self.apply_fill(c);
                            self.ctx.fill_rect(s as f64 * cw, y, w, line_thick);
                        }
                        run = Some((col_idx, fg, cell_w));
                    }
                    (_, None) => {
                        if let Some((s, c, w)) = run.take() {
                            self.apply_fill(c);
                            self.ctx.fill_rect(s as f64 * cw, y, w, line_thick);
                        }
                    }
                }
            }
            if let Some((s, c, w)) = run.take() {
                self.apply_fill(c);
                self.ctx.fill_rect(s as f64 * cw, y, w, line_thick);
            }
        }

        // Draw the cursor, but only when the viewport is at the bottom — in
        // scrollback the cursor is below the visible area and would render as
        // a stray block at the last row. Honor the shape set via DECSCUSR
        // (`\e[<n> q`) and DECTCEM (`\e[?25l` → Hidden); blur downgrades a
        // solid block to a hollow outline to match native Alacritty.
        if display_offset == 0 {
            let cursor = &content.cursor;
            // App-requested shape can be downgraded by focus state: an
            // unfocused window with a Block cursor renders as HollowBlock.
            let shape = if !self.focused && cursor.shape == CursorShape::Block {
                CursorShape::HollowBlock
            } else {
                cursor.shape
            };
            if shape != CursorShape::Hidden {
                let cursor_color = term_colors[NamedColor::Cursor]
                    .unwrap_or_else(|| colors::default_named_color(NamedColor::Cursor));
                let cursor_row = cursor.point.line.0 + display_offset;
                if cursor_row >= 0 && (cursor_row as usize) < screen_lines {
                    let cx = cursor.point.column.0 as f64 * cw;
                    let cy = cursor_row as f64 * ch;
                    // Roughly match native: beam ≈ 2px wide, underline ≈ 2px tall.
                    let bar = (ch / 10.0).max(2.0).round();
                    match shape {
                        CursorShape::Block => {
                            self.apply_fill(cursor_color);
                            self.ctx.fill_rect(cx, cy, cw, ch);
                            let crow = cursor_row as usize;
                            let ccol = cursor.point.column.0;
                            if crow < grid.len() && ccol < grid[crow].len() {
                                if let Some(cv) = &grid[crow][ccol] {
                                    if cv.ch != ' ' && cv.ch != '\0' {
                                        self.apply_fill(bg_color);
                                        let mut scratch = self.text_run.borrow_mut();
                                        scratch.clear();
                                        scratch.push(cv.ch);
                                        let _ = self.ctx.fill_text(&scratch, cx, cy + text_y_offset);
                                        scratch.clear();
                                    }
                                }
                            }
                        }
                        CursorShape::HollowBlock => {
                            // 1-pixel stroke inset by half a pixel so the line
                            // lands inside the cell.
                            self.apply_stroke(cursor_color);
                            self.ctx.set_line_width(1.0);
                            self.ctx.stroke_rect(cx + 0.5, cy + 0.5, cw - 1.0, ch - 1.0);
                        }
                        CursorShape::Beam => {
                            self.apply_fill(cursor_color);
                            self.ctx.fill_rect(cx, cy, bar, ch);
                        }
                        CursorShape::Underline => {
                            self.apply_fill(cursor_color);
                            self.ctx.fill_rect(cx, cy + ch - bar, cw, bar);
                        }
                        CursorShape::Hidden => unreachable!(),
                    }
                }
            }
        }

        // Bell overlay: a translucent white wash over the whole canvas. Drawn
        // last so it dims everything. Peak alpha kept modest (0.35) so the
        // flash is noticeable but not retina-searing; the render loop decays
        // `bell_intensity` toward zero across the next few frames.
        if self.bell_intensity > 0.0 {
            let alpha = (self.bell_intensity * 0.35).min(0.35);
            self.ctx.set_fill_style_str(&format!("rgba(255,255,255,{alpha})"));
            self.ctx.fill_rect(0.0, 0.0, canvas_width, canvas_height);
            // Setting fillStyle via raw string bypasses our cache; reset so
            // the next frame's apply_fill doesn't think the cache value is
            // still applied.
            self.current_fill.set(None);
        }
    }

    /// Set the font size and remeasure cell dimensions.
    pub fn set_font_size(&mut self, size_px: f32) {
        self.font_size_px = size_px;
        self.remeasure_cells();
    }

    /// Set the font family and remeasure cell dimensions.
    pub fn set_font_family(&mut self, family: &str) {
        self.font_family = family.to_string();
        self.remeasure_cells();
    }

    /// Set the line height multiplier and remeasure cell dimensions.
    pub fn set_line_height_multiplier(&mut self, multiplier: f32) {
        self.line_height_multiplier = multiplier;
        self.remeasure_cells();
    }

    /// Remeasure cell dimensions after font changes.
    fn remeasure_cells(&mut self) {
        let font_str = format!("{}px {}", self.font_size_px, self.font_family);
        self.ctx.set_font(&font_str);
        if let Ok(metrics) = self.ctx.measure_text("M") {
            self.cell_width = metrics.width() as f32;
            self.cell_height = (self.font_size_px * self.line_height_multiplier).ceil();
        }
    }

    /// Resize the canvas backing store to match the current CSS size,
    /// applying device-pixel-ratio scaling. Must be called from JS
    /// (e.g. a ResizeObserver) rather than from render(), because
    /// changing canvas.width triggers a synchronous reflow that can
    /// re-enter WASM.
    pub fn resize_backing_store(&self) {
        let dpr = self.device_pixel_ratio;
        let css_w = self.canvas.client_width().max(1) as u32;
        let css_h = self.canvas.client_height().max(1) as u32;
        let needed_w = (css_w as f64 * dpr).ceil() as u32;
        let needed_h = (css_h as f64 * dpr).ceil() as u32;
        if self.canvas.width() != needed_w || self.canvas.height() != needed_h {
            self.canvas.set_width(needed_w);
            self.canvas.set_height(needed_h);
            if let Err(e) = self.ctx.scale(dpr, dpr) {
                log::warn!("Canvas scale failed: {e:?}");
            }
            // Context state is reset by set_width/set_height.
            disable_kerning(&self.ctx);
            let font_str = format!("{}px {}", self.font_size_px, self.font_family);
            self.ctx.set_font(&font_str);
            self.ctx.set_text_baseline("top");
        }
    }
}

impl TerminalRenderer for Canvas2dRenderer {
    fn render(&mut self, term: &Term<WebEventProxy>) {
        Canvas2dRenderer::render(self, term);
    }

    fn resize_backing_store(&mut self) {
        Canvas2dRenderer::resize_backing_store(self);
    }

    fn cell_width(&self) -> f32 {
        self.cell_width
    }

    fn cell_height(&self) -> f32 {
        self.cell_height
    }

    fn set_font_size(&mut self, size_px: f32) {
        Canvas2dRenderer::set_font_size(self, size_px);
    }

    fn set_font_family(&mut self, family: &str) {
        Canvas2dRenderer::set_font_family(self, family);
    }

    fn set_line_height_multiplier(&mut self, multiplier: f32) {
        Canvas2dRenderer::set_line_height_multiplier(self, multiplier);
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn set_bell_intensity(&mut self, intensity: f32) {
        self.bell_intensity = intensity.clamp(0.0, 1.0);
    }

    fn backend_name(&self) -> &'static str {
        "canvas2d"
    }
}
