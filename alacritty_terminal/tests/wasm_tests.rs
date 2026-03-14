//! WASM-specific tests for alacritty_terminal.
//!
//! These tests verify that core terminal functionality works correctly
//! when compiled to wasm32. They run under wasm-pack test --node.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi;

/// A no-op event listener for tests.
#[derive(Copy, Clone)]
struct Mock;

impl EventListener for Mock {
    fn send_event(&self, _event: Event) {}
}

#[wasm_bindgen_test]
fn term_new_on_wasm32() {
    let size = TermSize::new(80, 24);
    let term = Term::new(Config::default(), &size, Mock);

    assert_eq!(term.grid().columns(), 80);
    assert_eq!(term.grid().screen_lines(), 24);
}

#[wasm_bindgen_test]
fn vte_parser_basic_escape_sequences() {
    let size = TermSize::new(80, 24);
    let mut term = Term::new(Config::default(), &size, Mock);
    let mut parser = ansi::Processor::new();

    // Feed "Hello" through the VTE parser.
    let input = b"Hello";
    parser.advance(&mut term, input);

    // The cursor should have advanced 5 columns.
    let cursor = term.grid().cursor.point;
    assert_eq!(cursor.column.0, 5);
    assert_eq!(cursor.line.0, 0);
}

#[wasm_bindgen_test]
fn vte_parser_csi_cursor_movement() {
    let size = TermSize::new(80, 24);
    let mut term = Term::new(Config::default(), &size, Mock);
    let mut parser = ansi::Processor::new();

    // Write text, then use CSI sequence to move cursor to position (1,1).
    // ESC [ H moves cursor to home (0,0 in 0-indexed).
    let input = b"Hello\x1b[H";
    parser.advance(&mut term, input);

    let cursor = term.grid().cursor.point;
    assert_eq!(cursor.column.0, 0);
    assert_eq!(cursor.line.0, 0);
}

#[wasm_bindgen_test]
fn fair_mutex_lock_unlock() {
    let mutex = FairMutex::new(42u32);

    // Lock and read value.
    {
        let guard = mutex.lock();
        assert_eq!(*guard, 42);
    }

    // Lock and mutate value.
    {
        let mut guard = mutex.lock();
        *guard = 100;
    }

    // Verify mutation persisted.
    {
        let guard = mutex.lock();
        assert_eq!(*guard, 100);
    }
}

#[wasm_bindgen_test]
fn terminal_resize() {
    let size = TermSize::new(80, 24);
    let mut term = Term::new(Config::default(), &size, Mock);

    assert_eq!(term.grid().columns(), 80);
    assert_eq!(term.grid().screen_lines(), 24);

    // Resize the terminal.
    let new_size = TermSize::new(120, 40);
    term.resize(new_size);

    assert_eq!(term.grid().columns(), 120);
    assert_eq!(term.grid().screen_lines(), 40);
}

#[wasm_bindgen_test]
fn vte_parser_newline_handling() {
    let size = TermSize::new(80, 24);
    let mut term = Term::new(Config::default(), &size, Mock);
    let mut parser = ansi::Processor::new();

    // Write text with newlines (LF moves cursor down, CR returns to column 0).
    let input = b"Line1\r\nLine2";
    parser.advance(&mut term, input);

    let cursor = term.grid().cursor.point;
    assert_eq!(cursor.line.0, 1);
    assert_eq!(cursor.column.0, 5); // "Line2" is 5 chars.
}

#[wasm_bindgen_test]
fn terminal_clear_screen() {
    let size = TermSize::new(80, 24);
    let mut term = Term::new(Config::default(), &size, Mock);
    let mut parser = ansi::Processor::new();

    // Write some text then clear screen (CSI 2 J) and go home (CSI H).
    let input = b"Hello World\x1b[2J\x1b[H";
    parser.advance(&mut term, input);

    let cursor = term.grid().cursor.point;
    assert_eq!(cursor.column.0, 0);
    assert_eq!(cursor.line.0, 0);
}
