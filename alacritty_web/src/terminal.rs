//! Web terminal state wrapper around alacritty_terminal::Term.

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config as TermConfig, TermMode};
use alacritty_terminal::Term;
use alacritty_terminal::vte::ansi;

use std::cell::RefCell;
use std::rc::Rc;

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

/// Event listener that collects events for the web frontend.
#[derive(Clone)]
pub struct WebEventProxy {
    /// Queue of clipboard events to be processed asynchronously.
    clipboard_events: Rc<RefCell<Vec<ClipboardEvent>>>,
    /// Queue of PTY write requests (from OSC 52 load, PtyWrite events, etc.).
    pty_writes: Rc<RefCell<Vec<String>>>,
}

impl WebEventProxy {
    pub fn new() -> Self {
        Self {
            clipboard_events: Rc::new(RefCell::new(Vec::new())),
            pty_writes: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Drain pending clipboard events.
    pub fn drain_clipboard_events(&self) -> Vec<ClipboardEvent> {
        self.clipboard_events.borrow_mut().drain(..).collect()
    }

    /// Drain pending PTY write requests.
    pub fn drain_pty_writes(&self) -> Vec<String> {
        self.pty_writes.borrow_mut().drain(..).collect()
    }
}

impl EventListener for WebEventProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => {
                // Terminal content changed, schedule a redraw.
            },
            Event::Title(title) => {
                log::info!("Terminal title: {title}");
            },
            Event::Bell => {
                log::debug!("Terminal bell");
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
}

impl WebTerminal {
    /// Create a new terminal with the given dimensions.
    pub fn new(cols: u16, lines: u16) -> Self {
        let event_proxy = WebEventProxy::new();
        let size = TermSize { lines: lines as usize, cols: cols as usize };
        let config = TermConfig::default();
        let term = Term::new(config, &size, event_proxy.clone());
        let term = Rc::new(FairMutex::new(term));

        Self {
            term,
            parser: ansi::Processor::new(),
            event_proxy,
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
}
