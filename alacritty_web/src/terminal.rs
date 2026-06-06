//! Web terminal state wrapper around alacritty_terminal::Term.

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config as TermConfig, TermMode};
use alacritty_terminal::Term;
use alacritty_terminal::index::Direction;
use alacritty_terminal::term::search::{RegexIter, RegexSearch};
use alacritty_terminal::vte::ansi;
use alacritty_terminal::vte::ansi::Rgb;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// Simple dimensions type for terminal sizing.
struct TermSize {
    lines: usize,
    cols: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// Pending clipboard events queued by the terminal for async processing.
#[derive(Clone, Debug)]
pub enum ClipboardEvent {
    /// Store text to the system clipboard.
    Store(String),
    /// Load clipboard contents (triggered by OSC 52).
    Load,
}

/// Maximum bell decay counter value. With a single BEL the renderer draws
/// `BELL_DECAY_MAX` frames of fading overlay; consecutive bells re-saturate
/// (so a stream of BELs still pulses visibly without infinitely accumulating).
const BELL_DECAY_MAX: u8 = 4;

/// Formatter handed in by `Event::ColorRequest` — takes the resolved colour
/// and returns the OSC reply string the shell expects.
pub type ColorReplyFormatter = Arc<dyn Fn(Rgb) -> String + Sync + Send + 'static>;

/// Event listener that collects events for the web frontend.
#[derive(Clone)]
pub struct WebEventProxy {
    /// Queue of clipboard events to be processed asynchronously.
    clipboard_events: Rc<RefCell<Vec<ClipboardEvent>>>,
    /// Queue of PTY write requests (from OSC 52 load, PtyWrite events, etc.).
    pty_writes: Rc<RefCell<Vec<String>>>,
    /// Latest OSC 0/2 window title. Updated as the shell sets it (so the
    /// last-write-wins). JS polls via `AlacrittyTerminal::title()` and mirrors
    /// the value into `document.title` if it wants the browser tab to track.
    title: Rc<RefCell<Option<String>>>,
    /// Bell decay counter — incremented to `BELL_DECAY_MAX` on each
    /// `Event::Bell`, decremented once per frame by the render loop. The
    /// renderer reads this to draw a brief overlay flash.
    bell_decay: Rc<Cell<u8>>,
    /// Pending OSC 4/10/11/12 colour queries from the shell. Each entry is
    /// the named-colour palette index plus the formatter the shell handed us
    /// to build the reply. The render loop drains these with the term lock
    /// held, looks up the current colour, calls the formatter, and forwards
    /// the reply bytes back over the PTY.
    color_requests: Rc<RefCell<Vec<(usize, ColorReplyFormatter)>>>,
}

impl Default for WebEventProxy {
    fn default() -> Self {
        Self::new()
    }
}

impl WebEventProxy {
    pub fn new() -> Self {
        Self {
            clipboard_events: Rc::new(RefCell::new(Vec::new())),
            pty_writes: Rc::new(RefCell::new(Vec::new())),
            title: Rc::new(RefCell::new(None)),
            bell_decay: Rc::new(Cell::new(0)),
            color_requests: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Drain pending colour query requests. Returned tuples are
    /// `(named_color_index, formatter)` — callers should look up the colour
    /// and invoke the formatter to build the OSC reply.
    pub fn drain_color_requests(&self) -> Vec<(usize, ColorReplyFormatter)> {
        self.color_requests.borrow_mut().drain(..).collect()
    }

    /// Drain pending clipboard events.
    pub fn drain_clipboard_events(&self) -> Vec<ClipboardEvent> {
        self.clipboard_events.borrow_mut().drain(..).collect()
    }

    /// Drain pending PTY write requests.
    pub fn drain_pty_writes(&self) -> Vec<String> {
        self.pty_writes.borrow_mut().drain(..).collect()
    }

    /// Latest OSC 0/2 title, or `None` if the shell never set one.
    pub fn title(&self) -> Option<String> {
        self.title.borrow().clone()
    }

    /// Bell intensity in `[0.0, 1.0]`. The render loop should call this
    /// per frame, draw an overlay at the returned intensity, then call
    /// `decay_bell()` once to advance the counter.
    pub fn bell_intensity(&self) -> f32 {
        self.bell_decay.get() as f32 / BELL_DECAY_MAX as f32
    }

    /// Step the bell decay counter towards zero. Called once per rendered
    /// frame; pairs with `bell_intensity()`.
    pub fn decay_bell(&self) {
        let v = self.bell_decay.get();
        if v > 0 {
            self.bell_decay.set(v - 1);
        }
    }

    /// Whether the bell is currently still decaying — JS uses this to decide
    /// whether to keep marking the terminal dirty across frames so the fade
    /// actually animates instead of stopping after one paint.
    pub fn bell_active(&self) -> bool {
        self.bell_decay.get() > 0
    }
}

impl EventListener for WebEventProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => {
                // Terminal content changed, schedule a redraw.
            },
            Event::Title(title) => {
                log::debug!("Terminal title: {title}");
                *self.title.borrow_mut() = Some(title);
            },
            Event::ResetTitle => {
                *self.title.borrow_mut() = None;
            },
            Event::Bell => {
                log::debug!("Terminal bell");
                self.bell_decay.set(BELL_DECAY_MAX);
            },
            Event::ClipboardStore(_ty, text) => {
                log::debug!("ClipboardStore: {} bytes", text.len());
                self.clipboard_events
                    .borrow_mut()
                    .push(ClipboardEvent::Store(text));
            },
            Event::ClipboardLoad(_ty, formatter) => {
                log::debug!("ClipboardLoad requested (OSC 52)");
                self.clipboard_events
                    .borrow_mut()
                    .push(ClipboardEvent::Load);
                // Read the clipboard asynchronously and format the response for the PTY.
                let pty_writes = self.pty_writes.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match read_clipboard().await {
                        Ok(text) => {
                            let formatted = formatter(&text);
                            pty_writes.borrow_mut().push(formatted);
                        },
                        Err(e) => {
                            log::warn!("Failed to read clipboard for OSC 52: {:?}", e);
                        },
                    }
                });
            },
            Event::PtyWrite(text) => {
                self.pty_writes.borrow_mut().push(text);
            },
            Event::ColorRequest(index, formatter) => {
                self.color_requests.borrow_mut().push((index, formatter));
            },
            _ => {},
        }
    }
}

/// Write text to the system clipboard using the browser Clipboard API.
pub async fn write_clipboard(text: &str) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("No window"))?;
    let navigator = window.navigator();
    let clipboard = navigator.clipboard();
    let promise = clipboard.write_text(text);
    JsFuture::from(promise).await?;
    Ok(())
}

/// Read text from the system clipboard using the browser Clipboard API.
pub async fn read_clipboard() -> Result<String, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("No window"))?;
    let navigator = window.navigator();
    let clipboard = navigator.clipboard();
    let promise = clipboard.read_text();
    let result = JsFuture::from(promise).await?;
    result
        .as_string()
        .ok_or_else(|| JsValue::from_str("Clipboard did not return a string"))
}

/// Wrapper around the terminal state for web usage.
pub struct WebTerminal {
    term: Rc<FairMutex<Term<WebEventProxy>>>,
    parser: ansi::Processor,
    event_proxy: WebEventProxy,
    /// Compiled regex from the last `set_search_pattern`. Kept on the
    /// terminal so `search_next` reuses it across calls.
    search_regex: Option<RegexSearch>,
}

impl WebTerminal {
    /// Create a new terminal with the given dimensions.
    pub fn new(cols: u16, lines: u16) -> Self {
        let event_proxy = WebEventProxy::new();
        let size = TermSize { lines: lines as usize, cols: cols as usize };
        // Opt into kitty keyboard protocol — without this, alacritty_terminal's
        // push_keyboard_mode is a no-op so DISAMBIGUATE_ESC_CODES never sticks.
        let mut config = TermConfig::default();
        config.kitty_keyboard = true;
        let term = Term::new(config, &size, event_proxy.clone());
        let term = Rc::new(FairMutex::new(term));

        Self {
            term,
            parser: ansi::Processor::new(),
            event_proxy,
            search_regex: None,
        }
    }

    /// Feed bytes from the PTY into the terminal parser.
    pub fn process_bytes(&mut self, bytes: &[u8]) {
        let mut term = self.term.lock();
        self.parser.advance(&mut *term, bytes);
    }

    /// Resize the terminal grid.
    pub fn resize(&mut self, cols: u16, lines: u16) {
        let size = TermSize { lines: lines as usize, cols: cols as usize };
        let mut term = self.term.lock();
        term.resize(size);
    }

    /// Get a reference to the terminal state for rendering.
    pub fn term(&self) -> &Rc<FairMutex<Term<WebEventProxy>>> {
        &self.term
    }

    /// Get the number of columns in the terminal grid.
    pub fn cols(&self) -> u16 {
        let term = self.term.lock();
        term.columns() as u16
    }

    /// Get the number of rows in the terminal grid.
    pub fn rows(&self) -> u16 {
        let term = self.term.lock();
        term.screen_lines() as u16
    }

    /// Get the event proxy for processing clipboard and PTY write events.
    pub fn event_proxy(&self) -> &WebEventProxy {
        &self.event_proxy
    }

    /// Get the current selection as a string, if any.
    pub fn selection_to_string(&self) -> Option<String> {
        let term = self.term.lock();
        term.selection_to_string()
    }

    /// Scroll the display viewport by `delta` lines. Positive scrolls into
    /// scrollback (towards older output), negative scrolls towards the bottom.
    pub fn scroll_display(&mut self, delta: i32) {
        let mut term = self.term.lock();
        term.scroll_display(Scroll::Delta(delta));
    }

    /// Jump the display viewport to the bottom (most recent output).
    pub fn scroll_to_bottom(&mut self) {
        let mut term = self.term.lock();
        term.scroll_display(Scroll::Bottom);
    }

    /// Start a new simple selection at the given viewport cell.
    /// `viewport_row` is 0..screen_lines, where 0 is the top of what's
    /// currently on screen (accounting for scrollback).
    pub fn selection_start(&mut self, viewport_row: i32, column: usize, side_left: bool) {
        let mut term = self.term.lock();
        let display_offset = term.grid().display_offset() as i32;
        let line_index = viewport_row - display_offset;
        let point = Point::new(Line(line_index), Column(column));
        let side = if side_left { Side::Left } else { Side::Right };
        term.selection = Some(Selection::new(SelectionType::Simple, point, side));
    }

    /// Start a block (rectangular) selection at the given viewport cell.
    /// Matches native Alacritty's Ctrl+Alt+drag behaviour — selects a
    /// rectangle of cells rather than a wrap-aware line range.
    pub fn selection_start_block(&mut self, viewport_row: i32, column: usize, side_left: bool) {
        let mut term = self.term.lock();
        let display_offset = term.grid().display_offset() as i32;
        let line_index = viewport_row - display_offset;
        let point = Point::new(Line(line_index), Column(column));
        let side = if side_left { Side::Left } else { Side::Right };
        term.selection = Some(Selection::new(SelectionType::Block, point, side));
    }

    /// Extend the active selection to the given viewport cell.
    pub fn selection_update(&mut self, viewport_row: i32, column: usize, side_left: bool) {
        let mut term = self.term.lock();
        let display_offset = term.grid().display_offset() as i32;
        let line_index = viewport_row - display_offset;
        let point = Point::new(Line(line_index), Column(column));
        let side = if side_left { Side::Left } else { Side::Right };
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    /// Clear any active selection.
    pub fn selection_clear(&mut self) {
        let mut term = self.term.lock();
        term.selection = None;
    }

    /// Select a word (via semantic boundaries) at the given viewport cell.
    pub fn selection_word(&mut self, viewport_row: i32, column: usize) {
        let mut term = self.term.lock();
        let display_offset = term.grid().display_offset() as i32;
        let line_index = viewport_row - display_offset;
        let point = Point::new(Line(line_index), Column(column));
        term.selection = Some(Selection::new(SelectionType::Semantic, point, Side::Left));
        if let Some(sel) = term.selection.as_mut() {
            sel.update(point, Side::Right);
        }
    }

    /// Select a whole line at the given viewport cell.
    pub fn selection_line(&mut self, viewport_row: i32, column: usize) {
        let mut term = self.term.lock();
        let display_offset = term.grid().display_offset() as i32;
        let line_index = viewport_row - display_offset;
        let point = Point::new(Line(line_index), Column(column));
        term.selection = Some(Selection::new(SelectionType::Lines, point, Side::Left));
        if let Some(sel) = term.selection.as_mut() {
            sel.update(point, Side::Right);
        }
    }

    /// Return the OSC 8 hyperlink URI of the cell at the given viewport
    /// position, or `None` if the cell carries no link. `viewport_row` is
    /// 0..screen_lines (top of viewport). Returned String is the raw URI as
    /// the shell pushed it — callers should treat untrusted, validate with
    /// `URL` before navigating.
    pub fn hyperlink_at(&self, viewport_row: i32, column: usize) -> Option<String> {
        let term = self.term.lock();
        let display_offset = term.grid().display_offset() as i32;
        let line_index = viewport_row - display_offset;
        let screen_lines = term.screen_lines() as i32;
        // Clamp to visible viewport plus scrollback. The point translation
        // handles negative lines (scrollback) — display_iter would yield
        // them — but we don't want to construct points outside the grid.
        if line_index < -(term.grid().history_size() as i32) || line_index >= screen_lines {
            return None;
        }
        let cols = term.columns();
        if column >= cols {
            return None;
        }
        let point = Point::new(Line(line_index), Column(column));
        term.grid()[point].hyperlink().map(|h| h.uri().to_string())
    }

    /// Compile a search regex. Returns true on success, false on a parse
    /// error (caller can use that to surface a UI hint). Subsequent
    /// `search_next` calls use this pattern. Passing an empty string
    /// clears the active pattern.
    pub fn set_search_pattern(&mut self, pattern: &str) -> bool {
        if pattern.is_empty() {
            self.search_regex = None;
            return true;
        }
        match RegexSearch::new(pattern) {
            Ok(rx) => {
                self.search_regex = Some(rx);
                true
            },
            Err(_) => false,
        }
    }

    /// Whether there's an active compiled search pattern.
    pub fn has_search_pattern(&self) -> bool {
        self.search_regex.is_some()
    }

    /// Enumerate every match of the active pattern between
    /// `(start_row, 0)` and `(end_row, last_col)` inclusive, in viewport
    /// coords. Returned vec is flat: each match contributes four entries
    /// `[start_row, start_col, end_row, end_col]`. JS slices it into
    /// 4-tuples.
    ///
    /// Returns `None` if no pattern is set. Returns `Some(vec![])` if a
    /// pattern is set but no matches were found.
    pub fn all_matches(&mut self, start_row: i32, end_row: i32) -> Option<Vec<i32>> {
        let regex = self.search_regex.as_mut()?;
        let term = self.term.lock();
        let display_offset = term.grid().display_offset() as i32;
        let cols = term.columns();
        let last_col = if cols == 0 { 0 } else { cols - 1 };
        let start = Point::new(Line(start_row - display_offset), Column(0));
        let end = Point::new(Line(end_row - display_offset), Column(last_col));
        let mut out = Vec::new();
        // RegexIter is `Right`-by-default for forward enumeration.
        let iter = RegexIter::new(start, end, Direction::Right, &*term, regex);
        for m in iter {
            let s = m.start();
            let e = m.end();
            out.push(s.line.0 + display_offset);
            out.push(s.column.0 as i32);
            out.push(e.line.0 + display_offset);
            out.push(e.column.0 as i32);
        }
        Some(out)
    }

    /// Find the next match of the active pattern starting from the given
    /// viewport cell. Returns `[start_row, start_col, end_row, end_col]`
    /// in viewport coordinates, or `None` if no match (or no pattern set).
    /// `forward = false` searches backward.
    pub fn search_next(
        &mut self,
        viewport_row: i32,
        column: u32,
        forward: bool,
    ) -> Option<Vec<i32>> {
        let regex = self.search_regex.as_mut()?;
        let term = self.term.lock();
        let display_offset = term.grid().display_offset() as i32;
        let line = Line(viewport_row - display_offset);
        let cols = term.columns();
        let col = Column((column as usize).min(cols.saturating_sub(1)));
        let origin = Point::new(line, col);
        let direction = if forward { Direction::Right } else { Direction::Left };
        let side = if forward { Side::Right } else { Side::Left };
        let m = term.search_next(regex, origin, direction, side, None)?;
        let start = m.start();
        let end = m.end();
        Some(vec![
            start.line.0 + display_offset,
            start.column.0 as i32,
            end.line.0 + display_offset,
            end.column.0 as i32,
        ])
    }

    /// Concatenated text of the cells on the given viewport row, with
    /// trailing spaces trimmed. Used by JS to regex-match URLs in plain
    /// text (e.g. on Ctrl+click) without exposing the full grid API.
    /// Returns `None` if the row is outside the grid.
    pub fn line_text(&self, viewport_row: i32) -> Option<String> {
        let term = self.term.lock();
        let display_offset = term.grid().display_offset() as i32;
        let line_index = viewport_row - display_offset;
        let screen_lines = term.screen_lines() as i32;
        if line_index < -(term.grid().history_size() as i32) || line_index >= screen_lines {
            return None;
        }
        let cols = term.columns();
        let mut out = String::with_capacity(cols);
        for col in 0..cols {
            let point = Point::new(Line(line_index), Column(col));
            out.push(term.grid()[point].c);
        }
        // Trim only trailing whitespace — leading spaces may be meaningful
        // (indented output, ASCII art) and middle spaces always are.
        while out.ends_with(' ') {
            out.pop();
        }
        Some(out)
    }

    /// Whether the terminal is currently in bracketed-paste mode.
    pub fn bracketed_paste(&self) -> bool {
        let term = self.term.lock();
        term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Packed mouse-reporting flags for the JS side to decide whether to
    /// forward mouse events to the PTY instead of starting a selection.
    /// Bit 0 = click reporting (DECSET 1000)
    /// Bit 1 = drag reporting  (DECSET 1002)
    /// Bit 2 = motion reporting (DECSET 1003)
    /// Bit 3 = SGR extended encoding (DECSET 1006)
    pub fn mouse_mode_bits(&self) -> u32 {
        let term = self.term.lock();
        let m = term.mode();
        let mut bits = 0u32;
        if m.contains(TermMode::MOUSE_REPORT_CLICK) { bits |= 1; }
        if m.contains(TermMode::MOUSE_DRAG)         { bits |= 2; }
        if m.contains(TermMode::MOUSE_MOTION)       { bits |= 4; }
        if m.contains(TermMode::SGR_MOUSE)          { bits |= 8; }
        bits
    }

    /// Current cursor line as it sits in the grid (signed; negative values
    /// would only appear if vi-mode scrolls the cursor into history, which
    /// we don't support yet — but the signed type matches `Point::line`).
    pub fn cursor_row(&self) -> i32 {
        let term = self.term.lock();
        term.grid().cursor.point.line.0
    }

    /// Current cursor column (zero-based).
    pub fn cursor_col(&self) -> u32 {
        let term = self.term.lock();
        term.grid().cursor.point.column.0 as u32
    }

    /// Packed keyboard-relevant mode flags. JS reads this once per keystroke
    /// instead of making four wasm calls.
    /// Bit 0 = APP_CURSOR (DECCKM, `\e[?1h`) — arrow keys send `\eOA` instead of `\e[A`
    /// Bit 1 = APP_KEYPAD (DECPAM)
    /// Bit 2 = FOCUS_IN_OUT (DECSET 1004) — host should send `\e[I` / `\e[O` on focus changes
    /// Bit 3 = DISAMBIGUATE_ESC_CODES (kitty keyboard `CSI > 1 u`) — emit CSI u
    ///         sequences for keys whose legacy encoding is ambiguous
    ///         (Ctrl+I vs Tab, Ctrl+M vs Enter, Ctrl+[ vs Escape, etc.)
    pub fn keyboard_mode_bits(&self) -> u32 {
        let term = self.term.lock();
        let m = term.mode();
        let mut bits = 0u32;
        if m.contains(TermMode::APP_CURSOR)             { bits |= 1; }
        if m.contains(TermMode::APP_KEYPAD)             { bits |= 2; }
        if m.contains(TermMode::FOCUS_IN_OUT)           { bits |= 4; }
        if m.contains(TermMode::DISAMBIGUATE_ESC_CODES) { bits |= 8; }
        bits
    }
}
