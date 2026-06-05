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
fn cursor_position_tracks_printable_writes() {
    // After printing "hello", the cursor sits one column past the last char.
    let mut t = WebTerminal::new(20, 4);
    assert_eq!(t.cursor_row(), 0);
    assert_eq!(t.cursor_col(), 0);
    t.process_bytes(b"hello");
    assert_eq!(t.cursor_row(), 0);
    assert_eq!(t.cursor_col(), 5);
    // CR then partial overwrite moves the column back and forward.
    t.process_bytes(b"\rxy");
    assert_eq!(t.cursor_col(), 2);
    // Newline advances the line and (for LNM-default) does NOT reset column.
    t.process_bytes(b"\r\nz");
    assert_eq!(t.cursor_row(), 1);
    assert_eq!(t.cursor_col(), 1);
}

#[test]
fn osc11_background_query_is_queued() {
    // `\e]11;?\x07` asks the terminal "what's your background colour?".
    // alacritty_terminal turns that into Event::ColorRequest, which the
    // proxy stashes onto its queue for the render loop to drain.
    let mut t = WebTerminal::new(20, 4);
    assert!(t.event_proxy().drain_color_requests().is_empty());
    t.process_bytes(b"\x1b]11;?\x07");
    let pending = t.event_proxy().drain_color_requests();
    assert_eq!(pending.len(), 1, "exactly one ColorRequest should be queued");
    // Second drain returns empty — drain consumes.
    assert!(t.event_proxy().drain_color_requests().is_empty());
}

#[test]
fn bracketed_paste_flag_tracks_decset_2004() {
    let mut t = WebTerminal::new(10, 4);
    assert!(!t.bracketed_paste());
    t.process_bytes(b"\x1b[?2004h");
    assert!(t.bracketed_paste(), "DECSET 2004 should enable bracketed paste");
    t.process_bytes(b"\x1b[?2004l");
    assert!(!t.bracketed_paste(), "DECRST 2004 should disable it");
}

#[test]
fn osc8_hyperlink_lookup_returns_uri() {
    // OSC 8 syntax is `\e]8;params;uri\e\\TEXT\e]8;;\e\\`. The cells holding
    // TEXT should carry the URI; cells outside the link should not.
    let mut t = WebTerminal::new(20, 4);
    t.process_bytes(b"foo \x1b]8;;https://example.com\x1b\\bar\x1b]8;;\x1b\\ baz");
    // "foo " spans cols 0-3 (no link), "bar" spans 4-6 (link), " baz" 7-10 (no link).
    assert_eq!(t.hyperlink_at(0, 0), None);
    assert_eq!(
        t.hyperlink_at(0, 4).as_deref(),
        Some("https://example.com"),
        "col 4 ('b' of bar) should carry the URI"
    );
    assert_eq!(
        t.hyperlink_at(0, 6).as_deref(),
        Some("https://example.com"),
        "col 6 ('r' of bar) should still carry the URI"
    );
    assert_eq!(t.hyperlink_at(0, 8), None, "col after the link is unlinked");
    // Out of bounds returns None rather than panicking.
    assert_eq!(t.hyperlink_at(0, 999), None);
    assert_eq!(t.hyperlink_at(999, 0), None);
}

#[test]
fn block_selection_returns_rectangular_text() {
    // Lay out a 5x3 grid of distinct chars, then block-select a 3-col band
    // starting at column 1. Expected slice across rows 0..2 is "BCD\nGHI\nLMN".
    let mut t = WebTerminal::new(5, 3);
    t.process_bytes(b"ABCDE\r\nFGHIJ\r\nKLMNO");
    t.selection_start_block(0, 1, true);
    t.selection_update(2, 3, false);
    let got = t.selection_to_string().expect("selection should produce text");
    assert_eq!(got, "BCD\nGHI\nLMN");
}

#[test]
fn bel_fires_bell_decay_counter() {
    // BEL (0x07) should bump the bell decay counter to its peak, and one
    // decay step should drop intensity below 1.0 without going negative.
    let mut t = WebTerminal::new(10, 4);
    assert_eq!(t.event_proxy().bell_intensity(), 0.0);
    assert!(!t.event_proxy().bell_active());
    t.process_bytes(b"\x07");
    let peak = t.event_proxy().bell_intensity();
    assert!(peak >= 0.99, "bell_intensity should peak at ~1.0, got {peak}");
    assert!(t.event_proxy().bell_active());
    t.event_proxy().decay_bell();
    let after = t.event_proxy().bell_intensity();
    assert!(after < peak && after >= 0.0, "decay should reduce intensity, got {after}");
}

#[test]
fn osc_title_is_captured_and_resettable() {
    // OSC 2 ; <title> ST sets the title; OSC 2 ; ST (empty) resets it.
    let mut t = WebTerminal::new(20, 4);
    assert_eq!(t.event_proxy().title(), None);
    t.process_bytes(b"\x1b]2;my-shell\x07");
    assert_eq!(t.event_proxy().title().as_deref(), Some("my-shell"));
    // A second title overwrites.
    t.process_bytes(b"\x1b]0;~/projects/foo\x07");
    assert_eq!(t.event_proxy().title().as_deref(), Some("~/projects/foo"));
}

#[test]
fn keyboard_mode_bits_track_app_cursor_and_focus() {
    // DECCKM (CSI ? 1 h) sets APP_CURSOR; DECPAM (ESC =) sets APP_KEYPAD;
    // DECSET 1004 enables focus reporting.
    let mut t = WebTerminal::new(10, 4);
    assert_eq!(t.keyboard_mode_bits(), 0);
    t.process_bytes(b"\x1b[?1h");
    assert!(t.keyboard_mode_bits() & 1 != 0, "app-cursor bit set after DECCKM");
    t.process_bytes(b"\x1b=");
    assert!(t.keyboard_mode_bits() & 2 != 0, "app-keypad bit set after DECPAM");
    t.process_bytes(b"\x1b[?1004h");
    assert!(t.keyboard_mode_bits() & 4 != 0, "focus bit set after 1004h");
    // DECRST should clear them.
    t.process_bytes(b"\x1b[?1l\x1b>\x1b[?1004l");
    assert_eq!(t.keyboard_mode_bits(), 0, "all keyboard mode bits cleared");
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
