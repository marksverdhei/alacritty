//! Web terminal state wrapper around alacritty_terminal::Term.

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Config as TermConfig;
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
}
