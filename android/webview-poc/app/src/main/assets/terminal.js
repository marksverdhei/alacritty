// Vanilla-JS bridge between an HTMLCanvasElement and the alacritty_web wasm
// bundle. Designed to be loaded from file:///android_asset/ in an Android
// WebView, but works equally well from any plain HTTP host (no bundler,
// no framework). Mirrors the input/mouse/clipboard logic of
// demo/src/lib/components/AlacrittyTerminal.svelte without the Svelte runes.
//
// Host bridge contract (when running inside the Android WebView):
//   window.AlacrittyHost.wsUrl()        ← JS → host: returns ws:// URL of the
//                                        bundled pty server once it's listening
//   window.AlacrittyHost.ready()        ← JS → host: terminal is mounted
//   window.__alacrittyInput(text)       ← host → JS: synthesised keystrokes
//                                        (e.g. Back button → ESC)
// Resize information rides the wasm bundle's own WebSocket (MSG_RESIZE byte
// in the binary protocol) — it does NOT go through the JS bridge.

import init, { AlacrittyTerminal } from './pkg/alacritty_web.js';

// ----- key mapping (ported from demo/src/lib/key-mapping.ts) -----------------

function mapKeyToBytes(e) {
  if (e.ctrlKey && e.key.length === 1) {
    const ch = e.key.toUpperCase();
    const c = ch.charCodeAt(0);
    if (c >= 65 && c <= 90) return new Uint8Array([c - 64]);
    if (ch === ' ' || ch === '@') return new Uint8Array([0]);
    switch (ch) {
      case '[': return new Uint8Array([0x1b]);
      case '\\': return new Uint8Array([0x1c]);
      case ']': return new Uint8Array([0x1d]);
      case '^': return new Uint8Array([0x1e]);
      case '_': return new Uint8Array([0x1f]);
      case '?': return new Uint8Array([0x7f]);
    }
  }
  switch (e.key) {
    case 'Enter': return new Uint8Array([13]);
    case 'Backspace': return new Uint8Array([127]);
    case 'Tab': return new Uint8Array([9]);
    case 'Escape': return new Uint8Array([27]);
    case 'ArrowUp': return new Uint8Array([27, 91, 65]);
    case 'ArrowDown': return new Uint8Array([27, 91, 66]);
    case 'ArrowRight': return new Uint8Array([27, 91, 67]);
    case 'ArrowLeft': return new Uint8Array([27, 91, 68]);
    case 'Home': return new Uint8Array([27, 91, 72]);
    case 'End': return new Uint8Array([27, 91, 70]);
    case 'PageUp': return new Uint8Array([27, 91, 53, 126]);
    case 'PageDown': return new Uint8Array([27, 91, 54, 126]);
    case 'Insert': return new Uint8Array([27, 91, 50, 126]);
    case 'Delete': return new Uint8Array([27, 91, 51, 126]);
    case 'F1': return new Uint8Array([27, 79, 80]);
    case 'F2': return new Uint8Array([27, 79, 81]);
    case 'F3': return new Uint8Array([27, 79, 82]);
    case 'F4': return new Uint8Array([27, 79, 83]);
    case 'F5': return new Uint8Array([27, 91, 49, 53, 126]);
    case 'F6': return new Uint8Array([27, 91, 49, 55, 126]);
    case 'F7': return new Uint8Array([27, 91, 49, 56, 126]);
    case 'F8': return new Uint8Array([27, 91, 49, 57, 126]);
    case 'F9': return new Uint8Array([27, 91, 50, 48, 126]);
    case 'F10': return new Uint8Array([27, 91, 50, 49, 126]);
    case 'F11': return new Uint8Array([27, 91, 50, 51, 126]);
    case 'F12': return new Uint8Array([27, 91, 50, 52, 126]);
  }
  if (e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) {
    return new TextEncoder().encode(e.key);
  }
  if (e.altKey && e.key.length === 1) {
    return new Uint8Array([27, ...new TextEncoder().encode(e.key)]);
  }
  return null;
}

// Decode a base64 PTY chunk delivered from the Kotlin host into bytes.
// `addJavascriptInterface` can't pass raw byte arrays, so the host base64s
// the chunk on its side and we round-trip here.
function bytesFromB64(b64) {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

// ----- attach -------------------------------------------------------------

/**
 * Mount the wasm terminal onto a canvas + wire up all the input handlers.
 *
 * @param {HTMLCanvasElement} canvas
 * @param {object} [opts]
 * @param {(bytes: Uint8Array) => void} [opts.onInput]   override the default
 *   input sink. When omitted the bytes are routed to a host bridge if one
 *   is present, otherwise echoed back into the terminal for local-only mode.
 * @returns {Promise<{terminal: AlacrittyTerminal, dispose: () => void}>}
 */
export async function attachTerminal(canvas, opts = {}) {
  await init();
  const terminal = new AlacrittyTerminal(canvas);
  const Host = (typeof window !== 'undefined' && window.AlacrittyHost) || null;

  // On Android, a focused <canvas> does NOT bring up the soft keyboard —
  // only form controls do. We park a 1×1 transparent <input> as a sibling
  // and route focus there instead. The keyboard pops up on focus, keydown
  // events still fire normally, and `input` events catch IME / autocomplete
  // text the keydown path would otherwise lose.
  const kbd = document.createElement('input');
  kbd.type = 'text';
  kbd.autocomplete = 'off';
  kbd.autocapitalize = 'none';
  kbd.autocorrect = 'off';
  kbd.spellcheck = false;
  kbd.setAttribute('aria-label', 'terminal input');
  Object.assign(kbd.style, {
    position: 'absolute',
    width: '1px',
    height: '1px',
    opacity: '0',
    border: 'none',
    padding: '0',
    margin: '0',
    pointerEvents: 'none',
    caretColor: 'transparent',
    // Place it near the canvas so screen readers anchor it correctly.
    top: '0',
    left: '0',
  });
  canvas.parentElement?.appendChild(kbd);

  // Focus router: any "focus the terminal" call should land on the input.
  // Canvas keeps focus styling via `terminal.set_focused()`.
  const focusTerminal = () => kbd.focus({ preventScroll: true });

  // Three input routes:
  //   1. opts.onInput supplied → caller takes the bytes (WebContainer demo etc).
  //   2. Host present          → terminal.write() — the wasm bundle's own
  //      WebSocket carries PTY input. This is the Android-WebView path.
  //   3. No Host               → local echo via feed() (desktop sanity check).
  const onInput = opts.onInput ?? ((bytes) => {
    if (Host) terminal.write(bytes);
    else terminal.feed(bytes);
  });

  function deliverInput(bytes) {
    terminal.scroll_to_bottom?.();
    terminal.selection_clear?.();
    onInput(bytes);
  }

  function deliverPaste(text) {
    if (!text) return;
    // wasm terminal's paste() handles bracketed-paste wrapping when the
    // term has DECSET 2004 on. But it also writes to its own WebSocket
    // which we don't have here, so we replicate just the bracketing.
    const bracketed = terminal.bracketed_paste?.();
    const enc = new TextEncoder();
    if (bracketed) {
      const start = new Uint8Array([0x1b, 0x5b, 0x32, 0x30, 0x30, 0x7e]); // ESC[200~
      const end   = new Uint8Array([0x1b, 0x5b, 0x32, 0x30, 0x31, 0x7e]); // ESC[201~
      const body  = enc.encode(text);
      const out = new Uint8Array(start.length + body.length + end.length);
      out.set(start, 0);
      out.set(body, start.length);
      out.set(end, start.length + body.length);
      deliverInput(out);
    } else {
      deliverInput(enc.encode(text));
    }
  }

  function modBits(e) {
    return (e.shiftKey ? 1 : 0) | ((e.altKey || e.metaKey) ? 2 : 0) | (e.ctrlKey ? 4 : 0);
  }

  function cellFromEvent(e) {
    const cellW = terminal.cell_width();
    const cellH = terminal.cell_height();
    if (cellW <= 0 || cellH <= 0) return null;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const col = Math.max(0, Math.min(terminal.cols() - 1, Math.floor(x / cellW)));
    const row = Math.max(0, Math.min(terminal.rows() - 1, Math.floor(y / cellH)));
    const inCellX = x - col * cellW;
    return { row, col, sideLeft: inCellX < cellW / 2 };
  }

  // --- keyboard ---
  // Soft-keyboard quirk: Android IMEs often emit keydown with `key:"Unidentified"`
  // for letter taps, deferring the actual character to the next `input` event.
  // We try keydown first (catches hardware keys + desktop typing); if
  // mapKeyToBytes can't handle it, we drop through and let the input event
  // do the work below.
  function handleKeydown(e) {
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && (e.key === 'C' || e.key === 'c')) {
      const text = terminal.selection_text?.();
      if (text) {
        navigator.clipboard?.writeText(text).catch(() => {});
        e.preventDefault();
        return;
      }
    }
    const isPaste =
      ((e.ctrlKey || e.metaKey) && e.shiftKey && (e.key === 'V' || e.key === 'v')) ||
      (e.metaKey && !e.shiftKey && (e.key === 'V' || e.key === 'v'));
    if (isPaste) {
      e.preventDefault();
      navigator.clipboard?.readText().then((text) => deliverPaste(text)).catch(() => {});
      return;
    }
    const bytes = mapKeyToBytes(e);
    if (bytes) {
      e.preventDefault();
      deliverInput(bytes);
    }
  }

  // IME composition state. `insertCompositionText` fires incrementally
  // with the growing buffer ("a", "ab", "abc") — sending each one would
  // duplicate characters in the terminal. We gate it: while composing,
  // swallow the input events; deliver the final string on compositionend.
  let composing = false;

  // Soft-keyboard text path. Handles non-composition keystrokes (English
  // typing, Backspace, line break) and pasted text from the system menu.
  function handleInput(e) {
    if (composing) {
      // Composing — final text will arrive on compositionend.
      kbd.value = '';
      return;
    }
    const t = e.inputType || '';
    if (t === 'insertText' || t === 'insertFromPaste') {
      if (e.data) deliverInput(new TextEncoder().encode(e.data));
    } else if (t === 'deleteContentBackward') {
      deliverInput(new Uint8Array([0x7f])); // ^?
    } else if (t === 'insertLineBreak') {
      deliverInput(new Uint8Array([13]));
    }
    // Clear the input so its buffer doesn't accumulate.
    kbd.value = '';
  }

  function handleCompositionStart() { composing = true; }
  function handleCompositionEnd(e) {
    composing = false;
    if (e.data) deliverInput(new TextEncoder().encode(e.data));
    kbd.value = '';
  }

  // --- wheel (scrollback + reported wheel) ---
  function handleWheel(e) {
    if (terminal.mouse_reporting_active?.()) {
      const cell = cellFromEvent(e);
      if (cell) {
        const direction = e.deltaY < 0 ? 64 : 65;
        const bytes = terminal.report_mouse(direction, 0, cell.col, cell.row, modBits(e));
        if (bytes) {
          e.preventDefault();
          deliverInput(bytes);
          return;
        }
      }
    }
    const lineHeight = terminal.cell_height?.() || 16;
    let lines = e.deltaMode === 1 ? e.deltaY : e.deltaY / lineHeight;
    const delta = -Math.round(lines * 3);
    if (delta === 0) return;
    e.preventDefault();
    terminal.scroll(delta);
  }

  // --- mouse: selection + reported clicks/drags ---
  let dragging = false;
  let reportedButton = null;

  function handleMouseDown(e) {
    // Browser-synthesised mousedown from a touch tap would otherwise start
    // a one-cell selection that the mouseup then copies to clipboard.
    if (isSyntheticFromTouch()) return;
    const cell = cellFromEvent(e);
    if (!cell) return;
    if (terminal.mouse_reporting_active?.() && !e.shiftKey) {
      const button = e.button === 1 ? 1 : e.button === 2 ? 2 : 0;
      const bytes = terminal.report_mouse(button, 0, cell.col, cell.row, modBits(e));
      if (bytes) {
        e.preventDefault();
        deliverInput(bytes);
        reportedButton = button;
        focusTerminal();
        return;
      }
    }
    if (e.button !== 0) return;
    if (e.detail >= 3) {
      terminal.selection_line(cell.row, cell.col);
      const text = terminal.selection_text();
      if (text) navigator.clipboard?.writeText(text).catch(() => {});
      dragging = false;
    } else if (e.detail === 2) {
      terminal.selection_word(cell.row, cell.col);
      const text = terminal.selection_text();
      if (text) navigator.clipboard?.writeText(text).catch(() => {});
      dragging = false;
    } else {
      terminal.selection_start(cell.row, cell.col, cell.sideLeft);
      dragging = true;
    }
    focusTerminal();
  }

  function handleMouseMove(e) {
    if (isSyntheticFromTouch()) return;
    if (terminal.mouse_reporting_active?.()) {
      const cell = cellFromEvent(e);
      if (!cell) return;
      const button = reportedButton ?? 3;
      const bytes = terminal.report_mouse(button, 2, cell.col, cell.row, modBits(e));
      if (bytes) {
        deliverInput(bytes);
        return;
      }
    }
    if (!dragging) return;
    const cell = cellFromEvent(e);
    if (!cell) return;
    terminal.selection_update(cell.row, cell.col, cell.sideLeft);
  }

  function handleMouseUp(e) {
    if (isSyntheticFromTouch()) return;
    if (reportedButton !== null) {
      const cell = cellFromEvent(e);
      if (cell) {
        const bytes = terminal.report_mouse(reportedButton, 1, cell.col, cell.row, modBits(e));
        if (bytes) deliverInput(bytes);
      }
      reportedButton = null;
      return;
    }
    if (!dragging) return;
    dragging = false;
    const text = terminal.selection_text();
    if (text) navigator.clipboard?.writeText(text).catch(() => {});
  }

  function handleContextMenu(e) {
    if (terminal.mouse_reporting_active?.() && !e.shiftKey) e.preventDefault();
  }
  function handleFocus() { terminal.set_focused(true); }
  function handleBlur()  { terminal.set_focused(false); }

  // --- touch: one-finger vertical drag → scrollback, tap → focus ---
  // Browsers synthesise mouse events from touch, so the existing mouse path
  // covers single-tap selection on phones. What it does NOT cover is wheel
  // (no synthetic wheel from a finger swipe) — so a real Android device
  // can't scroll the scrollback without this handler. We track touch in
  // pixel-y deltas and emit terminal.scroll() in cell-row units.
  let touchY = null;
  let touchAccumRows = 0;
  // Window during which we ignore synthesised mouse events that follow a
  // touch sequence. Without this, every tap on Android fires mousedown +
  // mouseup → terminal.selection_start + selection_text → clipboard gets
  // an empty/garbled string. 500ms covers the slowest browsers' tap delay.
  let lastTouchAt = 0;
  const isSyntheticFromTouch = () => performance.now() - lastTouchAt < 500;

  function handleTouchStart(e) {
    lastTouchAt = performance.now();
    if (e.touches.length !== 1) { touchY = null; return; }
    touchY = e.touches[0].clientY;
    touchAccumRows = 0;
    focusTerminal();
  }
  function handleTouchMove(e) {
    lastTouchAt = performance.now();
    if (touchY === null || e.touches.length !== 1) return;
    const cellH = terminal.cell_height?.() || 16;
    const y = e.touches[0].clientY;
    const dy = y - touchY;
    // Same sign convention as wheel: dragging *down* with finger should
    // reveal *older* content (positive scroll delta into history).
    const rows = (dy / cellH);
    const whole = Math.trunc(touchAccumRows + rows);
    touchAccumRows = (touchAccumRows + rows) - whole;
    touchY = y;
    if (whole !== 0) {
      e.preventDefault();
      terminal.scroll(whole);
    }
  }
  function handleTouchEnd() {
    lastTouchAt = performance.now();
    touchY = null;
  }

  // Keyboard listeners go on the hidden <input> (kbd), not the canvas —
  // that's what brings up the Android soft keyboard. focus/blur on kbd
  // drive the cursor's solid-vs-hollow state.
  kbd.addEventListener('keydown', handleKeydown);
  kbd.addEventListener('input', handleInput);
  kbd.addEventListener('compositionstart', handleCompositionStart);
  kbd.addEventListener('compositionend', handleCompositionEnd);
  kbd.addEventListener('focus', handleFocus);
  kbd.addEventListener('blur', handleBlur);
  canvas.addEventListener('wheel', handleWheel, { passive: false });
  canvas.addEventListener('mousedown', handleMouseDown);
  canvas.addEventListener('contextmenu', handleContextMenu);
  canvas.addEventListener('touchstart', handleTouchStart, { passive: false });
  canvas.addEventListener('touchmove', handleTouchMove, { passive: false });
  canvas.addEventListener('touchend', handleTouchEnd);
  canvas.addEventListener('touchcancel', handleTouchEnd);
  window.addEventListener('mousemove', handleMouseMove);
  window.addEventListener('mouseup', handleMouseUp);
  terminal.set_focused(document.activeElement === kbd);

  // --- resize observer + initial size ---
  const applySize = () => {
    terminal.sync_canvas_size();
    const cw = terminal.cell_width();
    const ch = terminal.cell_height();
    if (cw <= 0 || ch <= 0) return;
    const cols = Math.max(1, Math.floor(canvas.clientWidth / cw));
    const rows = Math.max(1, Math.floor(canvas.clientHeight / ch));
    terminal.resize(cols, rows);
  };
  requestAnimationFrame(applySize);
  const ro = new ResizeObserver(applySize);
  ro.observe(canvas);

  // --- host → JS hooks ---
  // PTY output chunks (from server → terminal display).
  window.__alacrittyOnBytes = (b64) => {
    terminal.feed(bytesFromB64(b64));
  };
  // Synthesised keystrokes from the Android host (Back→ESC, hardware
  // function keys on a keyboard case, etc). Treated identically to
  // keyboard input — same scroll-to-bottom, same routing.
  window.__alacrittyInput = (text) => {
    if (typeof text === 'string' && text.length > 0) {
      deliverInput(new TextEncoder().encode(text));
    }
  };

  if (Host && typeof Host.ready === 'function') Host.ready();

  function dispose() {
    kbd.removeEventListener('keydown', handleKeydown);
    kbd.removeEventListener('input', handleInput);
    kbd.removeEventListener('compositionstart', handleCompositionStart);
    kbd.removeEventListener('compositionend', handleCompositionEnd);
    kbd.removeEventListener('focus', handleFocus);
    kbd.removeEventListener('blur', handleBlur);
    canvas.removeEventListener('wheel', handleWheel);
    canvas.removeEventListener('mousedown', handleMouseDown);
    canvas.removeEventListener('contextmenu', handleContextMenu);
    canvas.removeEventListener('touchstart', handleTouchStart);
    canvas.removeEventListener('touchmove', handleTouchMove);
    canvas.removeEventListener('touchend', handleTouchEnd);
    canvas.removeEventListener('touchcancel', handleTouchEnd);
    window.removeEventListener('mousemove', handleMouseMove);
    window.removeEventListener('mouseup', handleMouseUp);
    kbd.remove();
    ro.disconnect();
    delete window.__alacrittyOnBytes;
    delete window.__alacrittyInput;
    terminal.dispose?.();
  }

  return { terminal, dispose, hostConnected: !!Host, focus: focusTerminal };
}
