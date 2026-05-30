//! Native (non-wasm) smoke tests for the VTE parsing path.
//!
//! These run with plain `cargo test` — they don't need a headless browser.
//! They cover the bits of `WebTerminal::process_bytes` whose correctness is
//! independent of the canvas renderer (printable text, SGR colour state,
//! cursor movement). The end-to-end rendering path is covered by the
//! Playwright `stress.spec.ts` suite.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_web::terminal::WebTerminal;

/// Drain through `process_bytes` and lock the term to inspect grid state.
fn feed_and_read(input: &[u8]) -> Vec<String> {
    let mut t = WebTerminal::new(20, 4);
    t.process_bytes(input);
    let term = t.term().lock();
    let cols = term.columns();
    let rows = term.screen_lines();
    let grid = term.grid();
    let mut out = Vec::with_capacity(rows);
    for line_idx in 0..rows {
        let line: i32 = line_idx as i32;
        let mut row = String::with_capacity(cols);
        for col in 0..cols {
            let point = alacritty_terminal::index::Point::new(
                alacritty_terminal::index::Line(line),
                alacritty_terminal::index::Column(col),
            );
            row.push(grid[point].c);
        }
        out.push(row.trim_end().to_string());
    }
    out
}

#[test]
fn plain_text_lands_on_grid() {
    let rows = feed_and_read(b"hello");
    assert_eq!(rows[0], "hello");
}

#[test]
fn newline_advances_to_next_row() {
    let rows = feed_and_read(b"a\r\nb");
    assert_eq!(rows[0], "a");
    assert_eq!(rows[1], "b");
}

#[test]
fn sgr_bold_sets_cell_flag() {
    let mut t = WebTerminal::new(10, 2);
    // ESC [ 1 m  -> bold on, then a glyph, then ESC [ 0 m -> reset.
    t.process_bytes(b"\x1b[1mB\x1b[0m");
    let term = t.term().lock();
    let point = alacritty_terminal::index::Point::new(
        alacritty_terminal::index::Line(0),
        alacritty_terminal::index::Column(0),
    );
    let cell = &term.grid()[point];
    assert_eq!(cell.c, 'B');
    assert!(cell.flags.contains(Flags::BOLD), "B should be bold");
}

#[test]
fn carriage_return_resets_column() {
    let rows = feed_and_read(b"abc\rxy");
    // CR returns to col 0 without clearing — first two chars get overwritten.
    assert_eq!(rows[0], "xyc");
}

#[test]
fn mouse_mode_bits_track_decset_modes() {
    // DECSET 1000 = MOUSE_REPORT_CLICK, 1002 = MOUSE_DRAG,
    // 1003 = MOUSE_MOTION, 1006 = SGR_MOUSE.
    let mut t = WebTerminal::new(10, 4);
    assert_eq!(t.mouse_mode_bits(), 0);
    t.process_bytes(b"\x1b[?1000h");
    assert!(t.mouse_mode_bits() & 1 != 0, "click bit should be set after 1000h");
    t.process_bytes(b"\x1b[?1006h");
    assert!(t.mouse_mode_bits() & 8 != 0, "sgr bit should be set after 1006h");
    t.process_bytes(b"\x1b[?1006l\x1b[?1000l");
    assert_eq!(t.mouse_mode_bits(), 0, "modes should clear after DECRST");
}

#[test]
fn batched_feed_matches_individual_feeds() {
    // Verifies the perf-optimization invariant: combining many small chunks
    // into one `process_bytes` call must produce identical grid state.
    let pieces: &[&[u8]] = &[b"al", b"acrit", b"ty", b"\r\n", b"web"];

    let mut a = WebTerminal::new(20, 4);
    for p in pieces {
        a.process_bytes(p);
    }

    let mut b = WebTerminal::new(20, 4);
    let combined: Vec<u8> = pieces.iter().flat_map(|p| p.iter().copied()).collect();
    b.process_bytes(&combined);

    let ta = a.term().lock();
    let tb = b.term().lock();
    for line_idx in 0..ta.screen_lines() {
        for col in 0..ta.columns() {
            let p = alacritty_terminal::index::Point::new(
                alacritty_terminal::index::Line(line_idx as i32),
                alacritty_terminal::index::Column(col),
            );
            assert_eq!(ta.grid()[p].c, tb.grid()[p].c, "char at row={line_idx} col={col}");
        }
    }
}
