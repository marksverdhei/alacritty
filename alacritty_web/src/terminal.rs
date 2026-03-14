//! Web terminal state wrapper around alacritty_terminal::Term.

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::Term;
use alacritty_terminal::vte::ansi;

use std::rc::Rc;

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

/// Event listener that collects events for the web frontend.
#[derive(Clone)]
pub struct WebEventProxy {
    // Events are processed inline on WASM (single-threaded).
}

impl WebEventProxy {
    pub fn new() -> Self {
        Self {}
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
            _ => {},
        }
    }
}

/// Wrapper around the terminal state for web usage.
pub struct WebTerminal {
    term: Rc<FairMutex<Term<WebEventProxy>>>,
    parser: ansi::Processor,
}

impl WebTerminal {
    /// Create a new terminal with the given dimensions.
    pub fn new(cols: u16, lines: u16) -> Self {
        let event_proxy = WebEventProxy::new();
        let size = TermSize { lines: lines as usize, cols: cols as usize };
        let config = TermConfig::default();
        let term = Term::new(config, &size, event_proxy);
        let term = Rc::new(FairMutex::new(term));

        Self {
            term,
            parser: ansi::Processor::new(),
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
}
