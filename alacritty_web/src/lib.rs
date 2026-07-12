//! Alacritty Web - WASM terminal renderer.

mod renderer;
pub mod terminal;
mod websocket;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
#[cfg(feature = "wgpu")]
use wasm_bindgen::JsCast;
use web_sys::HtmlCanvasElement;
#[cfg(feature = "wgpu")]
use web_sys::HtmlElement;

use renderer::TerminalRenderer;

/// Font configuration for the terminal renderer, matching `FontConfig` in `canvas2d.rs`.
///
/// All fields have sensible defaults so the struct can be constructed with just
/// `FontConfig::default()`.
pub use renderer::canvas2d::FontConfig;

/// Initialize panic hook and logger for better WASM debugging.
fn init_wasm() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info).ok();
}

/// Shared state for the terminal + renderer.
struct AppState {
    renderer: Box<dyn TerminalRenderer>,
    terminal: terminal::WebTerminal,
    dirty: bool,
    ws: Option<websocket::WsConnection>,
    /// Data fed directly (replay/WebContainer), not via WebSocket. Kept as a
    /// single flat buffer so 256 `feed()` calls in a hot loop don't allocate
    /// 256 `Vec<u8>` instances or take 256 mutex locks on the term.
    local_data: Vec<u8>,
    /// Whether the hosting canvas currently has keyboard focus. Drives the
    /// solid-vs-hollow cursor shape like native Alacritty.
    focused: bool,
    /// Timing of the most recent RAF tick that actually did work
    /// (`Performance.now()` deltas, milliseconds).
    last_parse_ms: f64,
    last_render_ms: f64,
    /// Monotonic counter incremented after every successful render. The
    /// stress benchmark polls this to detect when a feed() has actually
    /// landed on screen — much more accurate than waiting N RAFs.
    frame_seq: u64,
}

/// The main Alacritty terminal component for the browser.
#[wasm_bindgen]
pub struct AlacrittyTerminal {
    state: Rc<RefCell<AppState>>,
    #[allow(dead_code)]
    canvas: HtmlCanvasElement,
    /// Cached cell metrics so JS getters (called from ResizeObserver during a
    /// render borrow) never need to touch the RefCell.
    cell_w_cache: Rc<Cell<f32>>,
    cell_h_cache: Rc<Cell<f32>>,
}

#[wasm_bindgen]
impl AlacrittyTerminal {
    /// Create a new terminal attached to the given canvas element.
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement) -> Result<AlacrittyTerminal, JsError> {
        init_wasm();
        log::info!("Initializing AlacrittyTerminal");

        let renderer: Box<dyn TerminalRenderer> =
            Box::new(renderer::canvas2d::Canvas2dRenderer::new(&canvas)?);
        let cell_w = renderer.cell_width();
        let cell_h = renderer.cell_height();

        let css_width = canvas.client_width().max(1) as f32;
        let css_height = canvas.client_height().max(1) as f32;
        let cols = (css_width / cell_w).floor().max(1.0) as u16;
        let rows = (css_height / cell_h).floor().max(1.0) as u16;

        log::info!("Initial grid: {cols}x{rows}");

        let terminal = terminal::WebTerminal::new(cols, rows);

        let state = Rc::new(RefCell::new(AppState {
            renderer,
            terminal,
            dirty: true,
            ws: None,
            local_data: Vec::with_capacity(64 * 1024),
            focused: true,
            last_parse_ms: 0.0,
            last_render_ms: 0.0,
            frame_seq: 0,
        }));

        let cell_w_cache = Rc::new(Cell::new(cell_w));
        let cell_h_cache = Rc::new(Cell::new(cell_h));

        let term = AlacrittyTerminal {
            state: state.clone(),
            canvas: canvas.clone(),
            cell_w_cache,
            cell_h_cache,
        };

        term.start_render_loop();

        // Try to upgrade to wgpu asynchronously.
        // Only attempt if explicitly enabled via data attribute on canvas AND
        // the binary was compiled with `--features wgpu`.
        #[cfg(feature = "wgpu")]
        {
            let try_wgpu = canvas
                .get_attribute("data-enable-wgpu")
                .map(|v| v == "true")
                .unwrap_or(false);
            if try_wgpu {
                Self::try_upgrade_to_wgpu(state, canvas);
            } else {
                log::info!("wgpu upgrade not requested, using Canvas 2D");
            }
        }
        #[cfg(not(feature = "wgpu"))]
        {
            let _ = state;
            let _ = canvas;
        }

        Ok(term)
    }

    /// Connect to a WebSocket PTY server.
    pub fn connect(&mut self, ws_url: &str) -> Result<(), JsError> {
        let ws = websocket::WsConnection::new(ws_url)?;
        self.state.borrow_mut().ws = Some(ws);
        Ok(())
    }

    /// Connect to a WebSocket PTY server that requires `--token` auth.
    ///
    /// The PTY server expects the token as the first WebSocket text message,
    /// before any binary protocol frames. Pending resizes and writes stay
    /// queued behind this auth message until the socket opens.
    pub fn connect_with_token(&mut self, ws_url: &str, token: &str) -> Result<(), JsError> {
        let ws = websocket::WsConnection::new_with_token(ws_url, token)?;
        self.state.borrow_mut().ws = Some(ws);
        Ok(())
    }

    /// Disconnect from the PTY server.
    pub fn disconnect(&mut self) {
        self.state.borrow_mut().ws = None;
    }

    /// Current state of the underlying WebSocket, mapped from `WebSocket.readyState`.
    /// Returns 0 = connecting, 1 = open, 2 = closing, 3 = closed, -1 = no socket.
    pub fn ws_ready_state(&self) -> i32 {
        let Ok(app) = self.state.try_borrow() else {
            return -1;
        };
        app.ws.as_ref().map_or(-1, |ws| ws.ready_state() as i32)
    }

    /// Feed data directly into the terminal (for replay/local input, no PTY).
    /// Appended to a flat buffer so a burst of small calls turns into a single
    /// `parser.advance()` invocation in the next RAF.
    pub fn feed(&self, data: &[u8]) {
        if let Ok(mut app) = self.state.try_borrow_mut() {
            app.local_data.extend_from_slice(data);
            app.dirty = true;
        }
    }

    /// Write data to the PTY (send input). Also snaps the viewport back to
    /// the bottom and clears any active selection -- matches native Alacritty.
    pub fn write(&self, data: &[u8]) {
        if let Ok(mut app) = self.state.try_borrow_mut() {
            if let Some(ws) = &mut app.ws {
                ws.send_pty_data(data);
            }
            app.terminal.scroll_to_bottom();
            app.terminal.selection_clear();
            app.dirty = true;
        }
    }

    /// Send a resize message to the server.
    pub fn resize(&self, cols: u16, rows: u16) {
        // `try_borrow_mut` rather than `borrow_mut`: ResizeObserver can fire
        // while the render loop is mid-frame; ignoring this tick is fine
        // because the next frame will observe the new size.
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        app.terminal.resize(cols, rows);
        app.renderer.resize_backing_store();
        app.dirty = true;
        if let Some(ws) = &mut app.ws {
            ws.send_resize(cols, rows, 0, 0);
        }
    }

    /// Resize the canvas backing store to match its CSS size. Call this
    /// whenever the canvas element's size changes (e.g. from ResizeObserver).
    pub fn sync_canvas_size(&self) {
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        app.renderer.resize_backing_store();
        app.dirty = true;
    }

    /// Get cell width in pixels. Cached so this is safe to call re-entrantly
    /// (e.g. from a ResizeObserver while a render borrow is active).
    pub fn cell_width(&self) -> f32 {
        self.cell_w_cache.get()
    }

    /// Get cell height in pixels. Cached -- see `cell_width` for why.
    pub fn cell_height(&self) -> f32 {
        self.cell_h_cache.get()
    }

    /// Set the font size in pixels and trigger a re-render.
    pub fn set_font_size(&self, size_px: f32) {
        let mut app = self.state.borrow_mut();
        app.renderer.set_font_size(size_px);
        self.cell_w_cache.set(app.renderer.cell_width());
        self.cell_h_cache.set(app.renderer.cell_height());
        app.dirty = true;
    }

    /// Set the font family and trigger a re-render.
    pub fn set_font_family(&self, family: &str) {
        let mut app = self.state.borrow_mut();
        app.renderer.set_font_family(family);
        self.cell_w_cache.set(app.renderer.cell_width());
        self.cell_h_cache.set(app.renderer.cell_height());
        app.dirty = true;
    }

    /// Set the line height multiplier and trigger a re-render.
    pub fn set_line_height_multiplier(&self, multiplier: f32) {
        let mut app = self.state.borrow_mut();
        app.renderer.set_line_height_multiplier(multiplier);
        self.cell_w_cache.set(app.renderer.cell_width());
        self.cell_h_cache.set(app.renderer.cell_height());
        app.dirty = true;
    }

    /// Get the number of columns in the terminal grid.
    pub fn cols(&self) -> u16 {
        self.state.borrow().terminal.cols()
    }

    /// Get the number of rows in the terminal grid.
    pub fn rows(&self) -> u16 {
        self.state.borrow().terminal.rows()
    }

    /// Get the active renderer backend name ("wgpu" or "canvas2d").
    pub fn renderer_backend(&self) -> String {
        self.state.borrow().renderer.backend_name().to_string()
    }

    /// Wall time spent draining/parsing PTY bytes during the most recent
    /// RAF that did work, in milliseconds. Zero when no data was processed.
    pub fn last_parse_ms(&self) -> f64 {
        self.state.try_borrow().map(|a| a.last_parse_ms).unwrap_or(0.0)
    }

    /// Wall time spent in the renderer (paint into canvas) during the most
    /// recent RAF that did work, in milliseconds.
    pub fn last_render_ms(&self) -> f64 {
        self.state.try_borrow().map(|a| a.last_render_ms).unwrap_or(0.0)
    }

    /// Monotonic counter — incremented once after every successful render.
    /// JS can poll this to know when a `feed()` has actually made it to
    /// screen, instead of guessing how many RAFs to wait.
    pub fn frame_seq(&self) -> u32 {
        // u32 is plenty (overflows after ~2 years at 60Hz); easier to bridge
        // to JS than u64, which serde-wasm-bindgen turns into BigInt.
        self.state.try_borrow().map(|a| a.frame_seq as u32).unwrap_or(0)
    }

    /// Number of fed-but-not-yet-parsed bytes. JS polls this to know when a
    /// feed() has been consumed by the next RAF.
    pub fn pending_bytes(&self) -> u32 {
        self.state
            .try_borrow()
            .map(|a| a.local_data.len() as u32)
            .unwrap_or(0)
    }

    /// Back-compat alias for the older bench name. Returns 0 if no pending,
    /// nonzero if pending — same semantics the benchmark cares about.
    pub fn pending_chunks(&self) -> u32 {
        self.pending_bytes()
    }

    /// Scroll the display viewport by `delta` lines. Positive scrolls into
    /// scrollback (towards older output), negative scrolls towards the bottom.
    pub fn scroll(&self, delta: i32) {
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        app.terminal.scroll_display(delta);
        app.dirty = true;
    }

    /// Jump the display viewport to the bottom (most recent output).
    pub fn scroll_to_bottom(&self) {
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        app.terminal.scroll_to_bottom();
        app.dirty = true;
    }

    /// Apply a palette override taken from a user's alacritty config. Each
    /// entry is a 7-char "#RRGGBB" string. Unknown keys are ignored. Any
    /// missing key falls back to the built-in default. Accepts the keys:
    ///   background, foreground, cursor,
    ///   black, red, green, yellow, blue, magenta, cyan, white,
    ///   bright_black, bright_red, bright_green, bright_yellow,
    ///   bright_blue, bright_magenta, bright_cyan, bright_white.
    pub fn set_palette(&self, palette: JsValue) -> Result<(), JsError> {
        // Use Handler::set_color which is the public path for poking colors
        // into a Term. The inherent `Term::colors` field is private.
        use alacritty_terminal::vte::ansi::{Handler, NamedColor, Rgb};
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return Err(JsError::new("terminal busy"));
        };
        let parse = |s: &str| -> Option<Rgb> {
            let s = s.trim().trim_start_matches('#');
            if s.len() != 6 { return None; }
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some(Rgb { r, g, b })
        };
        let entries: &[(&str, NamedColor)] = &[
            ("background", NamedColor::Background),
            ("foreground", NamedColor::Foreground),
            ("cursor", NamedColor::Cursor),
            ("black", NamedColor::Black),
            ("red", NamedColor::Red),
            ("green", NamedColor::Green),
            ("yellow", NamedColor::Yellow),
            ("blue", NamedColor::Blue),
            ("magenta", NamedColor::Magenta),
            ("cyan", NamedColor::Cyan),
            ("white", NamedColor::White),
            ("bright_black", NamedColor::BrightBlack),
            ("bright_red", NamedColor::BrightRed),
            ("bright_green", NamedColor::BrightGreen),
            ("bright_yellow", NamedColor::BrightYellow),
            ("bright_blue", NamedColor::BrightBlue),
            ("bright_magenta", NamedColor::BrightMagenta),
            ("bright_cyan", NamedColor::BrightCyan),
            ("bright_white", NamedColor::BrightWhite),
        ];
        let term = app.terminal.term().clone();
        let mut term_guard = term.lock();
        for (key, named) in entries {
            let v = js_sys::Reflect::get(&palette, &JsValue::from_str(key));
            if let Ok(v) = v {
                if let Some(s) = v.as_string() {
                    if let Some(rgb) = parse(&s) {
                        Handler::set_color(&mut *term_guard, *named as usize, rgb);
                    }
                }
            }
        }
        drop(term_guard);
        app.dirty = true;
        Ok(())
    }

    /// Set the focused state. Drives the solid-vs-hollow cursor shape.
    pub fn set_focused(&self, focused: bool) {
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        if app.focused != focused {
            app.focused = focused;
            app.dirty = true;
        }
    }

    /// Start a selection at the given viewport cell. `row` counts from the top
    /// of the visible area (0..rows). `side_left` selects whether the click
    /// landed on the left or right half of the cell.
    pub fn selection_start(&self, row: i32, column: u32, side_left: bool) {
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        app.terminal.selection_start(row, column as usize, side_left);
        app.dirty = true;
    }

    /// Start a block (rectangular) selection at the given viewport cell.
    /// JS dispatches to this on Ctrl+Alt+drag (Cmd+Alt on mac).
    pub fn selection_start_block(&self, row: i32, column: u32, side_left: bool) {
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        app.terminal.selection_start_block(row, column as usize, side_left);
        app.dirty = true;
    }

    /// Extend the active selection to the given viewport cell.
    pub fn selection_update(&self, row: i32, column: u32, side_left: bool) {
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        app.terminal.selection_update(row, column as usize, side_left);
        app.dirty = true;
    }

    /// Clear any active selection.
    pub fn selection_clear(&self) {
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        app.terminal.selection_clear();
        app.dirty = true;
    }

    /// Return the current selection as a string, or undefined if nothing is
    /// selected. Useful for the JS side to implement copy-to-clipboard.
    pub fn selection_text(&self) -> Option<String> {
        let app = self.state.try_borrow().ok()?;
        app.terminal.selection_to_string()
    }

    /// Select the word at the given viewport cell (semantic boundaries).
    pub fn selection_word(&self, row: i32, column: u32) {
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        app.terminal.selection_word(row, column as usize);
        app.dirty = true;
    }

    /// Select the entire line at the given viewport cell.
    pub fn selection_line(&self, row: i32, column: u32) {
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        app.terminal.selection_line(row, column as usize);
        app.dirty = true;
    }

    /// Encode a mouse event into a PTY byte sequence following the active
    /// terminal mouse-reporting mode (DECSET 1000/1002/1003/1006). Returns
    /// `None` when no mouse reporting is enabled OR the event should not be
    /// reported (e.g. a motion event when only click reporting is on).
    /// The JS side calls this from mousedown/mouseup/mousemove/wheel; if it
    /// gets bytes back it writes them to the PTY and skips its own default
    /// behaviour (start selection, scroll).
    ///
    /// Parameters:
    /// - `button`: canonical xterm button code. 0=left, 1=middle, 2=right,
    ///   3=released (used by legacy non-SGR encoding), 64=wheel up, 65=wheel
    ///   down. For "motion without button held" pass 3.
    /// - `action`: 0=press, 1=release, 2=motion.
    /// - `col`, `row`: zero-based grid coordinates inside the viewport.
    /// - `mods`: bit0=shift, bit1=alt/meta, bit2=ctrl.
    pub fn report_mouse(
        &self,
        button: u8,
        action: u8,
        col: u16,
        row: u16,
        mods: u8,
    ) -> Option<Vec<u8>> {
        let Ok(mut app) = self.state.try_borrow_mut() else { return None; };
        let bits = app.terminal.mouse_mode_bits();
        let click = bits & 1 != 0;
        let drag  = bits & 2 != 0;
        let motion = bits & 4 != 0;
        let sgr = bits & 8 != 0;
        if !(click || drag || motion) {
            return None;
        }
        // Filter motion events by mode. Native alacritty does the same gating:
        //   - drag mode: only motion *while a button is pressed*
        //   - motion mode: all motion
        //   - click-only: no motion at all
        if action == 2 {
            if !(motion || drag) { return None; }
            // The JS side encodes "no button held" as button = 3; drag mode
            // should ignore those.
            if drag && !motion && button == 3 { return None; }
        }

        // Build the modifier-adjusted button code. SGR encoding overlays
        // mods into the same byte; the legacy "M Cb Cx Cy" form puts the
        // shift/meta/ctrl bits at +4/+8/+16 too. Motion events ORd with 32
        // — that's how xterm signals "this is a drag, not a fresh click".
        let mod_shift = if mods & 1 != 0 { 4 } else { 0 };
        let mod_alt   = if mods & 2 != 0 { 8 } else { 0 };
        let mod_ctrl  = if mods & 4 != 0 { 16 } else { 0 };
        let motion_bit = if action == 2 { 32 } else { 0 };
        let base = button.saturating_add(mod_shift + mod_alt + mod_ctrl + motion_bit);

        // 1-based coords for the wire format.
        let col1 = col + 1;
        let row1 = row + 1;

        // Send write through the same path keystrokes use so it goes over the
        // WebSocket (or queues if not yet connected).
        let mut bytes: Vec<u8> = Vec::with_capacity(16);
        if sgr {
            let c = if action == 1 { 'm' } else { 'M' };
            // `\x1b[<{base};{col};{row}{c}`
            bytes.extend_from_slice(b"\x1b[<");
            bytes.extend_from_slice(base.to_string().as_bytes());
            bytes.push(b';');
            bytes.extend_from_slice(col1.to_string().as_bytes());
            bytes.push(b';');
            bytes.extend_from_slice(row1.to_string().as_bytes());
            bytes.push(c as u8);
        } else {
            // Legacy X10/Normal: max coords are 223 (255 - 32). Bail out
            // silently if we're outside the encodable range.
            if col1 > 223 || row1 > 223 { return None; }
            let btn = if action == 1 { 3 + mod_shift + mod_alt + mod_ctrl + motion_bit } else { base };
            bytes.extend_from_slice(b"\x1b[M");
            bytes.push(32u8.saturating_add(btn));
            bytes.push(32u8 + col1 as u8);
            bytes.push(32u8 + row1 as u8);
        }

        // Side-effect: ship to PTY immediately if connected. The Option<Vec<u8>>
        // return is also given back to JS so it can write via its own channel
        // (e.g. the `/compare` page owns its own WebSocket).
        if let Some(ws) = &mut app.ws {
            ws.send_pty_data(&bytes);
        }
        Some(bytes)
    }

    /// Whether the terminal currently wants mouse events forwarded to the PTY.
    /// JS reads this in mousedown to decide between starting a selection and
    /// calling `report_mouse`.
    pub fn mouse_reporting_active(&self) -> bool {
        self.state
            .try_borrow()
            .map(|a| a.terminal.mouse_mode_bits() != 0)
            .unwrap_or(false)
    }

    /// Current cursor row (grid line). Used by tests to verify
    /// readline / TUI movement behaviour through the live shell.
    pub fn cursor_row(&self) -> i32 {
        self.state.try_borrow().map(|a| a.terminal.cursor_row()).unwrap_or(0)
    }

    /// Current cursor column (zero-based).
    pub fn cursor_col(&self) -> u32 {
        self.state.try_borrow().map(|a| a.terminal.cursor_col()).unwrap_or(0)
    }

    /// Packed keyboard-mode flags for the JS side to consult on each keystroke.
    /// Bit 0 = APP_CURSOR (DECCKM) — arrows/Home/End use SS3 (`\eOA`) form
    /// Bit 1 = APP_KEYPAD (DECPAM)
    /// Bit 2 = FOCUS_IN_OUT (DECSET 1004) — host should emit `\e[I` / `\e[O`
    pub fn keyboard_mode_bits(&self) -> u32 {
        self.state
            .try_borrow()
            .map(|a| a.terminal.keyboard_mode_bits())
            .unwrap_or(0)
    }

    /// Whether the terminal is currently in bracketed-paste mode (DECSET 2004).
    /// JS reads this when sending pasted text so it can wrap with
    /// `\e[200~...\e[201~` to let shells distinguish typed from pasted bytes.
    pub fn bracketed_paste(&self) -> bool {
        self.state
            .try_borrow()
            .map(|a| a.terminal.bracketed_paste())
            .unwrap_or(false)
    }

    /// OSC 8 hyperlink URI at the given viewport cell, or `None`. JS calls
    /// this on mousemove to decide whether to switch the cursor to a pointer,
    /// and on click (with Ctrl/Cmd) to `window.open` the URI.
    pub fn hyperlink_at(&self, row: i32, column: u32) -> Option<String> {
        let app = self.state.try_borrow().ok()?;
        app.terminal.hyperlink_at(row, column as usize)
    }

    /// Concatenated text of the cells on the given viewport row, with
    /// trailing spaces trimmed. JS uses this on Ctrl+click to regex-match
    /// URLs in plain (non-OSC-8) text.
    pub fn line_text(&self, row: i32) -> Option<String> {
        let app = self.state.try_borrow().ok()?;
        app.terminal.line_text(row)
    }

    /// Compile a search pattern. `true` on success; `false` on regex parse
    /// error. Empty `pattern` clears the active search. Subsequent
    /// `search_next` calls use this compiled regex.
    pub fn set_search_pattern(&self, pattern: &str) -> bool {
        let Ok(mut app) = self.state.try_borrow_mut() else { return false };
        app.terminal.set_search_pattern(pattern)
    }

    /// Whether a search pattern is currently compiled.
    pub fn has_search_pattern(&self) -> bool {
        self.state
            .try_borrow()
            .map(|a| a.terminal.has_search_pattern())
            .unwrap_or(false)
    }

    /// Find the next match of the active pattern from `(row, column)`.
    /// `forward = false` searches backward. Returns
    /// `[start_row, start_col, end_row, end_col]` in viewport coords, or
    /// `None` if no pattern is set or no match was found.
    pub fn search_next(&self, row: i32, column: u32, forward: bool) -> Option<Vec<i32>> {
        let mut app = self.state.try_borrow_mut().ok()?;
        app.terminal.search_next(row, column, forward)
    }

    /// Enumerate every match of the active pattern in the given viewport
    /// row range, inclusive. Returns a flat `Vec<i32>` where every four
    /// entries are `[start_row, start_col, end_row, end_col]`. JS uses
    /// `.length / 4` for the match count and slices the rest for "X of N"
    /// indexing. Returns `None` if no pattern is set.
    pub fn all_matches(&self, start_row: i32, end_row: i32) -> Option<Vec<i32>> {
        let mut app = self.state.try_borrow_mut().ok()?;
        app.terminal.all_matches(start_row, end_row)
    }

    /// Set the search-match highlight overlay on the renderer. `ranges`
    /// is flat groups of 4 (start_row, start_col, end_row, end_col) in
    /// viewport coords; `current_index` is the index of the "current"
    /// match in those groups (drawn more prominently), or `-1` for none.
    /// Pass an empty `ranges` to clear the highlights.
    pub fn set_search_highlights(&self, ranges: Vec<i32>, current_index: i32) {
        let Ok(mut app) = self.state.try_borrow_mut() else { return };
        app.renderer.set_search_highlights(&ranges, current_index);
        app.dirty = true;
    }

    /// Latest OSC 0/2 window title the shell pushed, or `None` if never set.
    /// JS typically polls this on a low-frequency interval and mirrors it
    /// into `document.title`. Polling (vs. callbacks) keeps the JS↔WASM
    /// boundary simple — there's no closure ownership to track and JS can
    /// debounce as aggressively as it likes.
    pub fn title(&self) -> Option<String> {
        self.state.try_borrow().ok().and_then(|a| a.terminal.event_proxy().title())
    }

    /// Paste text into the PTY. Wraps the text with bracketed-paste markers
    /// when the terminal has requested that mode, so shells like bash can
    /// distinguish pasted bytes from typed bytes.
    pub fn paste(&self, text: &str) {
        let Ok(mut app) = self.state.try_borrow_mut() else {
            return;
        };
        let bracketed = app.terminal.bracketed_paste();
        let payload: Vec<u8> = if bracketed {
            let mut buf = Vec::with_capacity(text.len() + 12);
            buf.extend_from_slice(b"\x1b[200~");
            buf.extend_from_slice(text.as_bytes());
            buf.extend_from_slice(b"\x1b[201~");
            buf
        } else {
            text.as_bytes().to_vec()
        };
        if let Some(ws) = &mut app.ws {
            ws.send_pty_data(&payload);
        }
        app.terminal.scroll_to_bottom();
        app.terminal.selection_clear();
        app.dirty = true;
    }

    /// Clean up resources.
    pub fn dispose(self) {
        drop(self);
    }
}

impl AlacrittyTerminal {
    /// Check if the browser has a working WebGPU adapter.
    ///
    /// We intentionally do NOT fall back to WebGL2 via wgpu's GL backend
    /// because it causes WASM heap corruption on adapter creation failure.
    /// Returns a future that resolves to true only if navigator.gpu.requestAdapter()
    /// actually returns an adapter.
    #[cfg(feature = "wgpu")]
    async fn has_webgpu_adapter() -> bool {
        let Some(window) = web_sys::window() else {
            return false;
        };
        let navigator = window.navigator();
        let gpu = js_sys::Reflect::get(&navigator, &JsValue::from_str("gpu"));
        let Ok(gpu) = gpu else {
            return false;
        };
        if gpu.is_undefined() || gpu.is_null() {
            return false;
        }
        // Call navigator.gpu.requestAdapter() to check for a real adapter.
        let request_adapter =
            js_sys::Reflect::get(&gpu, &JsValue::from_str("requestAdapter"));
        let Ok(request_adapter) = request_adapter else {
            return false;
        };
        if !request_adapter.is_function() {
            return false;
        }
        let func: js_sys::Function = request_adapter.unchecked_into();
        let promise = func.call0(&gpu);
        let Ok(promise) = promise else {
            return false;
        };
        let promise: js_sys::Promise = promise.unchecked_into();
        let result = wasm_bindgen_futures::JsFuture::from(promise).await;
        match result {
            Ok(adapter) => !adapter.is_null() && !adapter.is_undefined(),
            Err(_) => false,
        }
    }

    /// Attempt to upgrade from Canvas 2D to wgpu renderer asynchronously.
    ///
    /// Creates a separate canvas for wgpu (since a canvas can only have one
    /// context type). On success, the wgpu canvas replaces the original and
    /// the Canvas 2D canvas is hidden. On failure, stays on Canvas 2D.
    #[cfg(feature = "wgpu")]
    fn try_upgrade_to_wgpu(state: Rc<RefCell<AppState>>, canvas: HtmlCanvasElement) {
        wasm_bindgen_futures::spawn_local(async move {
            // Check for a real WebGPU adapter before attempting wgpu init.
            if !Self::has_webgpu_adapter().await {
                log::info!("No WebGPU adapter available, staying on Canvas 2D");
                return;
            }
            // Create a sibling canvas for wgpu (can't reuse the 2d context canvas).
            let document = match web_sys::window().and_then(|w| w.document()) {
                Some(doc) => doc,
                None => return,
            };

            let wgpu_canvas: HtmlCanvasElement = match document
                .create_element("canvas")
                .ok()
                .and_then(|el| el.dyn_into::<HtmlCanvasElement>().ok())
            {
                Some(c) => c,
                None => {
                    log::warn!("Failed to create wgpu canvas element");
                    return;
                }
            };

            // Match the original canvas size and position.
            let wgpu_el: &HtmlElement = wgpu_canvas.unchecked_ref();
            let style = wgpu_el.style();
            let _ = style.set_property("position", "absolute");
            let _ = style.set_property("top", "0");
            let _ = style.set_property("left", "0");
            let _ = style.set_property("width", "100%");
            let _ = style.set_property("height", "100%");

            // Insert the wgpu canvas next to the original (hidden initially).
            let _ = style.set_property("display", "none");
            if let Some(parent) = canvas.parent_node() {
                if let Err(e) = parent.insert_before(&wgpu_canvas, Some(&canvas)) {
                    log::warn!("Failed to insert wgpu canvas: {e:?}");
                    return;
                }
            } else {
                log::warn!("Canvas has no parent node, cannot insert wgpu canvas");
                return;
            }

            // Try to initialize wgpu.
            match renderer::WgpuRenderer::new(&wgpu_canvas).await {
                Ok(wgpu_renderer) => {
                    if let Ok(mut app) = state.try_borrow_mut() {
                        app.renderer = Box::new(wgpu_renderer);
                        app.dirty = true;

                        // Show wgpu canvas, hide canvas2d canvas.
                        let wgpu_el: &HtmlElement = wgpu_canvas.unchecked_ref();
                        let _ = wgpu_el.style().set_property("display", "block");
                        let canvas_el: &HtmlElement = canvas.unchecked_ref();
                        let _ = canvas_el.style().set_property("display", "none");

                        log::info!("Upgraded to wgpu renderer");
                    }
                }
                Err(e) => {
                    // Clean up the unused wgpu canvas.
                    if let Some(parent) = wgpu_canvas.parent_node() {
                        let _ = parent.remove_child(&wgpu_canvas);
                    }
                    log::info!("wgpu not available, staying on Canvas 2D: {e:?}");
                }
            }
        });
    }

    /// Start the requestAnimationFrame render loop.
    fn start_render_loop(&self) {
        let state = self.state.clone();
        let callback = Rc::new(RefCell::new(None::<Closure<dyn FnMut()>>));
        let callback_clone = callback.clone();

        *callback.borrow_mut() = Some(Closure::wrap(Box::new(move || {
            // `Performance::now()` for sub-millisecond timing instrumentation.
            // Cheap (just a JS call) and only called when there's work to
            // measure, so it's fine to leave on in production.
            let perf = web_sys::window().and_then(|w| w.performance());

            // All data processing and rendering in a single borrow.
            if let Ok(mut app) = state.try_borrow_mut() {
                // Flush pending outgoing messages once the connection is open.
                if let Some(ws) = &mut app.ws {
                    ws.flush_pending();
                }

                let mut parse_ms = 0.0f64;
                let parse_start = perf.as_ref().map(|p| p.now());

                // Pull WS chunks into the same flat buffer as the locally-fed
                // data so we lock the term mutex and call into the VTE parser
                // exactly once per frame, no matter how many small chunks
                // arrived. The order (WS before local) matches the previous
                // behaviour.
                if let Some(ws) = &app.ws {
                    let ws_chunks = ws.drain_incoming();
                    if !ws_chunks.is_empty() {
                        // Reserve all at once to avoid repeated regrows.
                        let total: usize = ws_chunks.iter().map(|c| c.len()).sum();
                        app.local_data.reserve(total);
                        for chunk in &ws_chunks {
                            app.local_data.extend_from_slice(chunk);
                        }
                    }
                }

                if !app.local_data.is_empty() {
                    // Move the buffer out of `app` so the immutable borrow it
                    // returns from process_bytes (no, process_bytes takes &mut
                    // self) doesn't conflict. take() leaves an empty Vec
                    // behind; we put the capacity back below so subsequent
                    // frames don't re-allocate.
                    let buf = std::mem::take(&mut app.local_data);
                    app.terminal.process_bytes(&buf);
                    // Re-use the allocation across frames.
                    app.local_data = buf;
                    app.local_data.clear();
                    app.dirty = true;
                }

                if let (Some(p), Some(start)) = (perf.as_ref(), parse_start) {
                    parse_ms = p.now() - start;
                }

                // Answer any OSC 4/10/11/12 colour queries the shell pushed.
                // Drain BEFORE the render so the reply lands in pty_writes
                // before the WebSocket flush at the top of the next frame.
                let color_requests = app.terminal.event_proxy().drain_color_requests();
                if !color_requests.is_empty() {
                    use alacritty_terminal::vte::ansi::NamedColor;
                    let term = app.terminal.term().clone();
                    let term_guard = term.lock();
                    let term_colors = term_guard.colors();
                    let mut replies: Vec<String> = Vec::with_capacity(color_requests.len());
                    for (index, formatter) in color_requests {
                        // term.colors() carries user overrides (palette /
                        // OSC 4/10/11/12 sets); fall back to the named-color
                        // default for the first 16 + special slots.
                        let color = term_colors[index].or_else(|| {
                            // Indices 0..16 + the named-color slots have
                            // sensible defaults; beyond that we fall back to
                            // white rather than panic.
                            (index < 19)
                                .then(|| {
                                    let named = match index {
                                        0 => NamedColor::Black,
                                        1 => NamedColor::Red,
                                        2 => NamedColor::Green,
                                        3 => NamedColor::Yellow,
                                        4 => NamedColor::Blue,
                                        5 => NamedColor::Magenta,
                                        6 => NamedColor::Cyan,
                                        7 => NamedColor::White,
                                        8 => NamedColor::BrightBlack,
                                        9 => NamedColor::BrightRed,
                                        10 => NamedColor::BrightGreen,
                                        11 => NamedColor::BrightYellow,
                                        12 => NamedColor::BrightBlue,
                                        13 => NamedColor::BrightMagenta,
                                        14 => NamedColor::BrightCyan,
                                        15 => NamedColor::BrightWhite,
                                        16 => NamedColor::Foreground,
                                        17 => NamedColor::Background,
                                        18 => NamedColor::Cursor,
                                        _ => NamedColor::Foreground,
                                    };
                                    renderer::colors::default_named_color(named)
                                })
                        }).unwrap_or(alacritty_terminal::vte::ansi::Rgb { r: 255, g: 255, b: 255 });
                        replies.push(formatter(color));
                    }
                    drop(term_guard);
                    if let Some(ws) = &mut app.ws {
                        for reply in replies {
                            ws.send_pty_data(reply.as_bytes());
                        }
                    }
                }

                // Force a redraw while a BEL flash is decaying so the fade
                // actually animates instead of stopping after one frame.
                let bell_active = app.terminal.event_proxy().bell_active();
                if bell_active {
                    app.dirty = true;
                }

                // Render if dirty.
                if app.dirty {
                    let focused = app.focused;
                    app.renderer.set_focused(focused);
                    let bell_intensity = app.terminal.event_proxy().bell_intensity();
                    app.renderer.set_bell_intensity(bell_intensity);
                    let term = app.terminal.term().clone();
                    let term_guard = term.lock();
                    let render_start = perf.as_ref().map(|p| p.now());
                    app.renderer.render(&term_guard);
                    drop(term_guard);
                    let render_ms = match (perf.as_ref(), render_start) {
                        (Some(p), Some(start)) => p.now() - start,
                        _ => 0.0,
                    };
                    // Step the bell decay (now that this frame's intensity
                    // landed on the canvas). When this frame had a non-zero
                    // bell we mark dirty for the NEXT frame too so the final
                    // `intensity == 0` render actually paints — the canvas
                    // clear is what removes the overlay, not the decay alone.
                    let was_bell_frame = bell_intensity > 0.0;
                    if was_bell_frame {
                        app.terminal.event_proxy().decay_bell();
                    }
                    app.dirty = was_bell_frame;
                    app.last_parse_ms = parse_ms;
                    app.last_render_ms = render_ms;
                    app.frame_seq = app.frame_seq.wrapping_add(1);
                }
            }

            // Schedule next frame.
            if let Some(win) = web_sys::window() {
                if let Some(cb) = callback_clone.borrow().as_ref() {
                    if let Err(e) = win.request_animation_frame(cb.as_ref().unchecked_ref()) {
                        log::warn!("request_animation_frame failed: {e:?}");
                    }
                }
            }
        }) as Box<dyn FnMut()>));

        if let Some(win) = web_sys::window() {
            if let Some(cb) = callback.borrow().as_ref() {
                let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
            }
        }

        std::mem::forget(callback);
    }
}
