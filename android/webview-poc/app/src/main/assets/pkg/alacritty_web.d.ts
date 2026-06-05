/* tslint:disable */
/* eslint-disable */
/**
 * The main Alacritty terminal component for the browser.
 */
export class AlacrittyTerminal {
  free(): void;
  [Symbol.dispose](): void;
  /**
   * Get cell width in pixels. Cached so this is safe to call re-entrantly
   * (e.g. from a ResizeObserver while a render borrow is active).
   */
  cell_width(): number;
  /**
   * Disconnect from the PTY server.
   */
  disconnect(): void;
  /**
   * Get cell height in pixels. Cached -- see `cell_width` for why.
   */
  cell_height(): number;
  /**
   * Set the focused state. Drives the solid-vs-hollow cursor shape.
   */
  set_focused(focused: boolean): void;
  /**
   * Apply a palette override taken from a user's alacritty config. Each
   * entry is a 7-char "#RRGGBB" string. Unknown keys are ignored. Any
   * missing key falls back to the built-in default. Accepts the keys:
   *   background, foreground, cursor,
   *   black, red, green, yellow, blue, magenta, cyan, white,
   *   bright_black, bright_red, bright_green, bright_yellow,
   *   bright_blue, bright_magenta, bright_cyan, bright_white.
   */
  set_palette(palette: any): void;
  /**
   * Encode a mouse event into a PTY byte sequence following the active
   * terminal mouse-reporting mode (DECSET 1000/1002/1003/1006). Returns
   * `None` when no mouse reporting is enabled OR the event should not be
   * reported (e.g. a motion event when only click reporting is on).
   * The JS side calls this from mousedown/mouseup/mousemove/wheel; if it
   * gets bytes back it writes them to the PTY and skips its own default
   * behaviour (start selection, scroll).
   *
   * Parameters:
   * - `button`: canonical xterm button code. 0=left, 1=middle, 2=right,
   *   3=released (used by legacy non-SGR encoding), 64=wheel up, 65=wheel
   *   down. For "motion without button held" pass 3.
   * - `action`: 0=press, 1=release, 2=motion.
   * - `col`, `row`: zero-based grid coordinates inside the viewport.
   * - `mods`: bit0=shift, bit1=alt/meta, bit2=ctrl.
   */
  report_mouse(button: number, action: number, col: number, row: number, mods: number): Uint8Array | undefined;
  /**
   * Wall time spent draining/parsing PTY bytes during the most recent
   * RAF that did work, in milliseconds. Zero when no data was processed.
   */
  last_parse_ms(): number;
  /**
   * Number of fed-but-not-yet-parsed bytes. JS polls this to know when a
   * feed() has been consumed by the next RAF.
   */
  pending_bytes(): number;
  /**
   * Set the font size in pixels and trigger a re-render.
   */
  set_font_size(size_px: number): void;
  /**
   * Wall time spent in the renderer (paint into canvas) during the most
   * recent RAF that did work, in milliseconds.
   */
  last_render_ms(): number;
  /**
   * Back-compat alias for the older bench name. Returns 0 if no pending,
   * nonzero if pending — same semantics the benchmark cares about.
   */
  pending_chunks(): number;
  /**
   * Select the entire line at the given viewport cell.
   */
  selection_line(row: number, column: number): void;
  /**
   * Return the current selection as a string, or undefined if nothing is
   * selected. Useful for the JS side to implement copy-to-clipboard.
   */
  selection_text(): string | undefined;
  /**
   * Select the word at the given viewport cell (semantic boundaries).
   */
  selection_word(row: number, column: number): void;
  /**
   * Current state of the underlying WebSocket, mapped from `WebSocket.readyState`.
   * Returns 0 = connecting, 1 = open, 2 = closing, 3 = closed, -1 = no socket.
   */
  ws_ready_state(): number;
  /**
   * Clear any active selection.
   */
  selection_clear(): void;
  /**
   * Start a selection at the given viewport cell. `row` counts from the top
   * of the visible area (0..rows). `side_left` selects whether the click
   * landed on the left or right half of the cell.
   */
  selection_start(row: number, column: number, side_left: boolean): void;
  /**
   * Set the font family and trigger a re-render.
   */
  set_font_family(family: string): void;
  /**
   * Get the active renderer backend name ("wgpu" or "canvas2d").
   */
  renderer_backend(): string;
  /**
   * Jump the display viewport to the bottom (most recent output).
   */
  scroll_to_bottom(): void;
  /**
   * Extend the active selection to the given viewport cell.
   */
  selection_update(row: number, column: number, side_left: boolean): void;
  /**
   * Resize the canvas backing store to match its CSS size. Call this
   * whenever the canvas element's size changes (e.g. from ResizeObserver).
   */
  sync_canvas_size(): void;
  /**
   * Packed keyboard-mode flags for the JS side to consult on each keystroke.
   * Bit 0 = APP_CURSOR (DECCKM) — arrows/Home/End use SS3 (`\eOA`) form
   * Bit 1 = APP_KEYPAD (DECPAM)
   * Bit 2 = FOCUS_IN_OUT (DECSET 1004) — host should emit `\e[I` / `\e[O`
   */
  keyboard_mode_bits(): number;
  /**
   * Whether the terminal currently wants mouse events forwarded to the PTY.
   * JS reads this in mousedown to decide between starting a selection and
   * calling `report_mouse`.
   */
  mouse_reporting_active(): boolean;
  /**
   * Set the line height multiplier and trigger a re-render.
   */
  set_line_height_multiplier(multiplier: number): void;
  /**
   * Create a new terminal attached to the given canvas element.
   */
  constructor(canvas: HTMLCanvasElement);
  /**
   * Get the number of columns in the terminal grid.
   */
  cols(): number;
  /**
   * Feed data directly into the terminal (for replay/local input, no PTY).
   * Appended to a flat buffer so a burst of small calls turns into a single
   * `parser.advance()` invocation in the next RAF.
   */
  feed(data: Uint8Array): void;
  /**
   * Get the number of rows in the terminal grid.
   */
  rows(): number;
  /**
   * Paste text into the PTY. Wraps the text with bracketed-paste markers
   * when the terminal has requested that mode, so shells like bash can
   * distinguish pasted bytes from typed bytes.
   */
  paste(text: string): void;
  /**
   * Write data to the PTY (send input). Also snaps the viewport back to
   * the bottom and clears any active selection -- matches native Alacritty.
   */
  write(data: Uint8Array): void;
  /**
   * Send a resize message to the server.
   */
  resize(cols: number, rows: number): void;
  /**
   * Scroll the display viewport by `delta` lines. Positive scrolls into
   * scrollback (towards older output), negative scrolls towards the bottom.
   */
  scroll(delta: number): void;
  /**
   * Connect to a WebSocket PTY server.
   */
  connect(ws_url: string): void;
  /**
   * Clean up resources.
   */
  dispose(): void;
  /**
   * Monotonic counter — incremented once after every successful render.
   * JS can poll this to know when a `feed()` has actually made it to
   * screen, instead of guessing how many RAFs to wait.
   */
  frame_seq(): number;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_alacrittyterminal_free: (a: number, b: number) => void;
  readonly alacrittyterminal_cell_height: (a: number) => number;
  readonly alacrittyterminal_cell_width: (a: number) => number;
  readonly alacrittyterminal_cols: (a: number) => number;
  readonly alacrittyterminal_connect: (a: number, b: number, c: number) => [number, number];
  readonly alacrittyterminal_disconnect: (a: number) => void;
  readonly alacrittyterminal_dispose: (a: number) => void;
  readonly alacrittyterminal_feed: (a: number, b: number, c: number) => void;
  readonly alacrittyterminal_frame_seq: (a: number) => number;
  readonly alacrittyterminal_keyboard_mode_bits: (a: number) => number;
  readonly alacrittyterminal_last_parse_ms: (a: number) => number;
  readonly alacrittyterminal_last_render_ms: (a: number) => number;
  readonly alacrittyterminal_mouse_reporting_active: (a: number) => number;
  readonly alacrittyterminal_new: (a: any) => [number, number, number];
  readonly alacrittyterminal_paste: (a: number, b: number, c: number) => void;
  readonly alacrittyterminal_pending_bytes: (a: number) => number;
  readonly alacrittyterminal_pending_chunks: (a: number) => number;
  readonly alacrittyterminal_renderer_backend: (a: number) => [number, number];
  readonly alacrittyterminal_report_mouse: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
  readonly alacrittyterminal_resize: (a: number, b: number, c: number) => void;
  readonly alacrittyterminal_rows: (a: number) => number;
  readonly alacrittyterminal_scroll: (a: number, b: number) => void;
  readonly alacrittyterminal_scroll_to_bottom: (a: number) => void;
  readonly alacrittyterminal_selection_clear: (a: number) => void;
  readonly alacrittyterminal_selection_line: (a: number, b: number, c: number) => void;
  readonly alacrittyterminal_selection_start: (a: number, b: number, c: number, d: number) => void;
  readonly alacrittyterminal_selection_text: (a: number) => [number, number];
  readonly alacrittyterminal_selection_update: (a: number, b: number, c: number, d: number) => void;
  readonly alacrittyterminal_selection_word: (a: number, b: number, c: number) => void;
  readonly alacrittyterminal_set_focused: (a: number, b: number) => void;
  readonly alacrittyterminal_set_font_family: (a: number, b: number, c: number) => void;
  readonly alacrittyterminal_set_font_size: (a: number, b: number) => void;
  readonly alacrittyterminal_set_line_height_multiplier: (a: number, b: number) => void;
  readonly alacrittyterminal_set_palette: (a: number, b: any) => [number, number];
  readonly alacrittyterminal_sync_canvas_size: (a: number) => void;
  readonly alacrittyterminal_write: (a: number, b: number, c: number) => void;
  readonly alacrittyterminal_ws_ready_state: (a: number) => number;
  readonly __wbindgen_exn_store: (a: number) => void;
  readonly __externref_table_alloc: () => number;
  readonly __wbindgen_export_2: WebAssembly.Table;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_export_6: WebAssembly.Table;
  readonly __externref_table_dealloc: (a: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h5baad699b3dcff72: (a: number, b: number) => void;
  readonly closure129_externref_shim: (a: number, b: number, c: any) => void;
  readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;
/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
