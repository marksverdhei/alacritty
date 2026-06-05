# alacritty_web

WASM build of [alacritty_terminal](../alacritty_terminal) with a Canvas 2D
renderer, designed to embed a terminal in a browser. Parses the same VTE
escape-sequence stream that native Alacritty does (256/truecolor, 5 underline
styles, cursor shapes via DECSCUSR, OSC 8 hyperlinks, OSC 0/2 title, mouse
reporting, focus events, bracketed paste — see
[parity matrix](#feature-parity-vs-native-alacritty)) and renders the resulting
grid to an `HTMLCanvasElement`.

Bundle size: ~229 KB (wasm-opt optimized).

## Build

Requires `wasm-pack` and the `wasm32-unknown-unknown` Rust target.

```sh
rustup target add wasm32-unknown-unknown
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

# from the repo root:
wasm-pack build alacritty_web --target web --release --out-dir ../demo/static/pkg
```

This produces `pkg/alacritty_web.js` (ES module glue) and
`pkg/alacritty_web_bg.wasm` (the binary). Serve them as static assets.

### Optional `wgpu` feature

A `wgpu` Cargo feature exists but ships **disabled by default**. It used to be
the primary renderer; in practice the Canvas 2D path is competitive on
realistic payloads (parse time dominates, not render — see
`feedback_wasm_perf_bottleneck.md`). Building with `--features wgpu` adds the
`wgpu` + `bytemuck` deps and an additional ~2 MB to the wasm binary, so don't
enable unless you have a measured reason.

## Use from JavaScript

```js
import init, { AlacrittyTerminal } from './pkg/alacritty_web.js';

await init();
const canvas = document.querySelector('canvas');
const term = new AlacrittyTerminal(canvas);

// Pipe shell output into the parser.
term.feed(new TextEncoder().encode('hello \x1b[1mworld\x1b[0m\r\n'));

// Send keystrokes upstream. For an internal WebSocket:
term.connect('ws://localhost:7681');
term.write(new TextEncoder().encode('ls\r'));

// ...or skip connect() and route bytes yourself through term.feed() +
// your own transport (the /compare demo page does this so it can tee
// the same shell into two terminals).
```

> ⚠️ **`init()` must be called at most once per page load.** Calling it again
> silently rewires wasm-bindgen's shared `wasm` global so all previously-created
> `AlacrittyTerminal` handles misroute their method calls into the new
> instance. See `feedback_multi_instance_wasm_init.md`.

The demo's [`AlacrittyTerminal.svelte`](../demo/src/lib/components/AlacrittyTerminal.svelte)
component and [`canvas-handlers.ts`](../demo/src/lib/canvas-handlers.ts) are the
reference wiring — mouse, focus, keyboard, OSC 8 click, bracketed paste,
DECSET 1004 focus reporting.

## JS / TS API surface

Created with `new AlacrittyTerminal(canvas: HTMLCanvasElement)`. All methods
on the instance:

### Lifecycle / transport
| Method | Notes |
|---|---|
| `connect(wsUrl)` | Open an internal WebSocket. Drains incoming PTY bytes through `feed()` and routes `write()` outbound. Skip if you own the transport. |
| `disconnect()` | Close the internal WebSocket. |
| `ws_ready_state()` | `-1` if no socket, otherwise the WS `readyState`. |
| `dispose()` | Drop the instance. |
| `feed(bytes)` | Push bytes into the VTE parser (e.g. from your own WS, or a replay). |
| `write(bytes)` | Send bytes to the PTY via the internal WS. Side effects: scroll-to-bottom + clear selection (matches native). |
| `paste(text)` | Wraps with `\e[200~...\e[201~` if bracketed-paste mode is on, then sends. |

### Sizing / metrics
| Method | Returns |
|---|---|
| `resize(cols, rows)` | Resize the terminal grid + notify the PTY if connected. |
| `sync_canvas_size()` | Resync the canvas backing-store with its CSS size (call from a `ResizeObserver`). |
| `cell_width()` / `cell_height()` | CSS pixels per cell. |
| `cols()` / `rows()` | Current grid dimensions. |
| `cursor_row()` / `cursor_col()` | Cursor position in grid coords. |

### Rendering / theme
| Method | Notes |
|---|---|
| `set_font_size(px)` / `set_font_family(s)` / `set_line_height_multiplier(n)` | Live restyle; remeasures cell metrics. |
| `set_palette({...})` | Apply `#RRGGBB` overrides for `background`, `foreground`, `cursor`, the 8 named ANSI colors and their `bright_*` variants. |
| `set_focused(bool)` | Drives solid-vs-hollow cursor block. |
| `renderer_backend()` | `"canvas2d"` (or `"wgpu"` if compiled with that feature). |

### Selection
| Method | Notes |
|---|---|
| `selection_start(row, col, sideLeft)` / `selection_update(...)` / `selection_clear()` | Simple drag selection. |
| `selection_start_block(...)` | Rectangular (Alt+drag in the reference wiring). |
| `selection_word(row, col)` / `selection_line(row, col)` | Double-/triple-click semantics. |
| `selection_text()` | Returns the selected text, or `null`. |

### Scrolling
| Method | Notes |
|---|---|
| `scroll(delta)` | Positive = up into scrollback. |
| `scroll_to_bottom()` | Jump to most recent output. |

### Mouse reporting (DECSET 1000/1002/1003/1006)
| Method | Notes |
|---|---|
| `mouse_reporting_active()` | True when any reporting mode is on. JS calls this on mousedown to decide between selection and `report_mouse()`. |
| `report_mouse(button, action, col, row, mods)` | Returns the bytes to send to the PTY for a mouse event, encoded per active mode (SGR 1006 or legacy). `button`: 0/1/2 = L/M/R, 3 = no-button (motion), 64/65 = wheel up/down. `action`: 0=press, 1=release, 2=motion. `mods` bits: 1=shift, 2=alt, 4=ctrl. |

### Shell-facing introspection
| Method | Notes |
|---|---|
| `title()` | Latest OSC 0/2 title pushed by the shell, or `null`. Poll at ~4 Hz to mirror into `document.title`. |
| `hyperlink_at(row, col)` | OSC 8 URI at this cell, or `null`. JS uses this for hover cursor + Ctrl-click. |
| `bracketed_paste()` | True when the shell asked for bracketed paste (DECSET 2004). |
| `keyboard_mode_bits()` | Packed: bit 0 = APP_CURSOR (DECCKM), bit 1 = APP_KEYPAD (DECPAM), bit 2 = FOCUS_IN_OUT (DECSET 1004). Read on each keystroke. |

### Instrumentation
| Method | Notes |
|---|---|
| `last_parse_ms()` / `last_render_ms()` | Per-frame timing in milliseconds. |
| `frame_seq()` | Monotonic counter incremented after every render — poll to detect "feed() has landed on screen". |
| `pending_bytes()` | Fed-but-not-yet-parsed buffer length. |

## Feature parity vs native Alacritty

| | Status |
|---|---|
| VTE parsing (256/truecolor, SGR, OSC, DCS, CSI) | ✅ via `alacritty_terminal` |
| Cursor shapes (Block / Underline / Beam / HollowBlock / Hidden) | ✅ DECSCUSR + DECTCEM |
| Underline styles (single / double / dotted / dashed / curly) | ✅ |
| OSC 8 hyperlinks | ✅ rendered with underline + hover cursor + Ctrl-click to open |
| OSC 0/2 title | ✅ exposed via `title()` |
| OSC 4/10/11/12 color queries | ✅ answered back to PTY |
| Visual bell (BEL `\x07`) | ✅ 4-frame translucent flash |
| Mouse reporting (DECSET 1000/1002/1003) | ✅ via SGR 1006 |
| Focus events (DECSET 1004) | ✅ |
| Bracketed paste (DECSET 2004) | ✅ |
| Block (rectangular) selection | ✅ |
| Modified arrow keys / F-keys (xterm `CSI 1;m<letter>`) | ✅ |
| APP_CURSOR (DECCKM) arrow encoding | ✅ SS3 form |
| Scrollback | ✅ |
| Vi mode | ❌ |
| Regex search | ❌ |
| URL hint mode (auto-detect URLs in plain text) | ❌ (OSC 8 covers most cases) |
| Kitty keyboard protocol (CSI u) | ❌ |
| Sixel / Kitty graphics | ❌ (native doesn't either) |

## Tests

```sh
cargo test -p alacritty_web --tests
```

14 Rust unit tests covering VTE parsing surface + every wasm-bindgen getter
(title, hyperlink_at, keyboard_mode_bits, mouse_mode_bits, cursor pos, bell
decay, color queries, block selection, bracketed paste).

The browser-side tests live in `demo/tests/` (Playwright) — `keymap.spec.ts`
covers the JS key mapping matrix, `stress.spec.ts` covers rendering and
performance budgets, `live-pty.spec.ts` runs the full stack against a real
bash via `alacritty_pty_server`.
