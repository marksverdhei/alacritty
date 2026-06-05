/**
 * Shared mouse / wheel / focus / contextmenu wiring for the terminal canvas.
 *
 * Both `/` (via AlacrittyTerminal.svelte) and `/compare` (raw canvas + direct
 * wasm handle) used to maintain near-identical copies of these handlers.
 * Each copy independently grew OSC 8 hover, OSC 8 Ctrl+click, mouse reporting,
 * focus event reporting, contextmenu suppression, block selection, etc., and
 * I kept finding "feature on /, missing on /compare" gaps during the parity
 * sweep. Putting the bodies behind one entry point makes future feature
 * additions a one-place change.
 *
 * Keyboard handling stays per-route because the consumers have meaningfully
 * different paste / copy semantics (AlacrittyTerminal.svelte uses
 * `terminal.paste()`, /compare wraps with bracketed-paste markers and sends
 * via its own framed WebSocket).
 */

import { cellFromMouseEvent, modBits } from './terminal-input';

/**
 * The wasm-bindgen `AlacrittyTerminal` surface we touch. Typed structurally
 * so the helper is testable with stubs and not coupled to wasm-pack's
 * generated `.d.ts` types.
 */
export interface TerminalHandle {
	cell_width: () => number;
	cell_height: () => number;
	cols: () => number;
	rows: () => number;
	set_focused: (focused: boolean) => void;
	mouse_reporting_active?: () => boolean;
	report_mouse?: (button: number, action: number, col: number, row: number, mods: number) => Uint8Array | null | undefined;
	keyboard_mode_bits?: () => number;
	hyperlink_at?: (row: number, col: number) => string | null | undefined;
	selection_start: (row: number, col: number, sideLeft: boolean) => void;
	selection_start_block: (row: number, col: number, sideLeft: boolean) => void;
	selection_update: (row: number, col: number, sideLeft: boolean) => void;
	selection_word: (row: number, col: number) => void;
	selection_line: (row: number, col: number) => void;
	selection_text: () => string | null | undefined;
	scroll: (delta: number) => void;
}

export interface WireOpts {
	/** PTY transport. Bytes pushed here go to the shell. */
	sendInput: (bytes: Uint8Array) => void;
	/**
	 * Called on wheel events when mouse-reporting is OFF — lets the caller
	 * mirror scroll into another view (e.g. xterm.js on /compare). Default:
	 * call `terminal.scroll(delta)` directly.
	 */
	onScrollDelta?: (lineDelta: number) => void;
	/**
	 * URL-scheme allow-list for OSC 8 Ctrl/Cmd+click. Defaults to common
	 * safe-to-open schemes; intentionally excludes `javascript:` and `data:`.
	 */
	urlSchemeAllowlist?: RegExp;
}

const DEFAULT_URL_ALLOWLIST = /^(https?|file|mailto):/i;

/**
 * Wire all non-keyboard canvas event listeners. Returns a cleanup function
 * that removes every listener registered here.
 *
 * Inside this function:
 *   - `dragging` tracks an active selection drag
 *   - `reportedButton` tracks the button held during a mouse-reporting drag
 * Both are closure-local so multiple `wireTerminalCanvas` calls don't
 * collide on shared globals.
 */
export function wireTerminalCanvas(
	canvas: HTMLCanvasElement,
	terminal: TerminalHandle,
	opts: WireOpts,
): () => void {
	const send = opts.sendInput;
	const allowlist = opts.urlSchemeAllowlist ?? DEFAULT_URL_ALLOWLIST;
	let dragging = false;
	let reportedButton: number | null = null;

	const onMouseDown = (e: MouseEvent) => {
		const c = cellFromMouseEvent(e, canvas, terminal);
		if (!c) return;
		// OSC 8: Ctrl/Cmd+left-click opens the link.
		if (e.button === 0 && (e.ctrlKey || e.metaKey)) {
			const uri = terminal.hyperlink_at?.(c.row, c.col);
			if (uri) {
				e.preventDefault();
				try {
					if (allowlist.test(uri)) {
						window.open(uri, '_blank', 'noopener,noreferrer');
					}
				} catch {
					/* opener blocked */
				}
				return;
			}
		}
		// Mouse-reporting apps (vim/htop) take priority on any button. Shift
		// bypasses, matching the xterm "force browser selection" convention.
		if (terminal.mouse_reporting_active?.() && !e.shiftKey) {
			const button = e.button === 1 ? 1 : e.button === 2 ? 2 : 0;
			const bytes = terminal.report_mouse?.(button, 0, c.col, c.row, modBits(e));
			if (bytes) {
				e.preventDefault();
				send(bytes);
				reportedButton = button;
				canvas.focus({ preventScroll: true });
				return;
			}
		}
		// Left button only past this point — selection is left-button.
		if (e.button !== 0) return;
		if (e.detail >= 3) {
			terminal.selection_line(c.row, c.col);
			const t = terminal.selection_text();
			if (t) navigator.clipboard?.writeText(t).catch(() => {});
			dragging = false;
		} else if (e.detail === 2) {
			terminal.selection_word(c.row, c.col);
			const t = terminal.selection_text();
			if (t) navigator.clipboard?.writeText(t).catch(() => {});
			dragging = false;
		} else if (e.altKey) {
			// Alt+drag → rectangular (block) selection.
			terminal.selection_start_block(c.row, c.col, c.sideLeft);
			dragging = true;
		} else {
			terminal.selection_start(c.row, c.col, c.sideLeft);
			dragging = true;
		}
		canvas.focus({ preventScroll: true });
	};

	const onMouseMove = (e: MouseEvent) => {
		// Mouse-reporting motion: report_mouse rejects motion events the mode
		// hasn't asked for, so we always offer and discard the empty result.
		if (terminal.mouse_reporting_active?.()) {
			const c = cellFromMouseEvent(e, canvas, terminal);
			if (c) {
				const button = reportedButton ?? 3; // 3 = no button held
				const bytes = terminal.report_mouse?.(button, 2, c.col, c.row, modBits(e));
				if (bytes) {
					send(bytes);
					return;
				}
			}
		}
		if (!dragging) {
			// OSC 8 hover: pointer cursor on hyperlinked cells.
			const c = cellFromMouseEvent(e, canvas, terminal);
			if (c) {
				const uri = terminal.hyperlink_at?.(c.row, c.col);
				const wantPointer = !!uri;
				const isPointer = canvas.style.cursor === 'pointer';
				if (wantPointer && !isPointer) canvas.style.cursor = 'pointer';
				else if (!wantPointer && isPointer) canvas.style.cursor = '';
			}
			return;
		}
		const c = cellFromMouseEvent(e, canvas, terminal);
		if (!c) return;
		terminal.selection_update(c.row, c.col, c.sideLeft);
	};

	const onMouseUp = (e: MouseEvent) => {
		// Mouse-reporting release: forward and clear held-button state.
		if (reportedButton !== null) {
			const c = cellFromMouseEvent(e, canvas, terminal);
			if (c) {
				const bytes = terminal.report_mouse?.(reportedButton, 1, c.col, c.row, modBits(e));
				if (bytes) send(bytes);
			}
			reportedButton = null;
			return;
		}
		if (!dragging) return;
		dragging = false;
		const text = terminal.selection_text();
		if (text) navigator.clipboard?.writeText(text).catch(() => {});
	};

	const onWheel = (e: WheelEvent) => {
		// Mouse-reporting apps want wheel as button 64/65, not scrollback.
		if (terminal.mouse_reporting_active?.()) {
			const c = cellFromMouseEvent(e as unknown as MouseEvent, canvas, terminal);
			if (c) {
				const direction = e.deltaY < 0 ? 64 : 65;
				const bytes = terminal.report_mouse?.(direction, 0, c.col, c.row, modBits(e));
				if (bytes) {
					e.preventDefault();
					send(bytes);
					return;
				}
			}
		}
		const lineHeight = terminal.cell_height() || 16;
		const lines = e.deltaMode === 1 ? e.deltaY : e.deltaY / lineHeight;
		const delta = -Math.round(lines * 3);
		if (delta === 0) return;
		e.preventDefault();
		if (opts.onScrollDelta) opts.onScrollDelta(delta);
		else terminal.scroll(delta);
	};

	const onContextMenu = (e: MouseEvent) => {
		// Pass right-clicks to the running app when reporting is on; Shift+
		// right-click is the standard escape back to the browser menu.
		if (terminal.mouse_reporting_active?.() && !e.shiftKey) {
			e.preventDefault();
		}
	};

	const onFocus = () => {
		terminal.set_focused(true);
		// DECSET 1004: tell tmux/neovim that we just gained focus.
		if (((terminal.keyboard_mode_bits?.() ?? 0) & 4) !== 0) {
			send(new Uint8Array([0x1b, 0x5b, 0x49])); // ESC [ I
		}
	};

	const onBlur = () => {
		terminal.set_focused(false);
		if (((terminal.keyboard_mode_bits?.() ?? 0) & 4) !== 0) {
			send(new Uint8Array([0x1b, 0x5b, 0x4f])); // ESC [ O
		}
	};

	canvas.addEventListener('mousedown', onMouseDown);
	canvas.addEventListener('wheel', onWheel, { passive: false });
	canvas.addEventListener('contextmenu', onContextMenu);
	canvas.addEventListener('focus', onFocus);
	canvas.addEventListener('blur', onBlur);
	// move/up on window so dragging off-canvas still tracks.
	window.addEventListener('mousemove', onMouseMove);
	window.addEventListener('mouseup', onMouseUp);

	return () => {
		canvas.removeEventListener('mousedown', onMouseDown);
		canvas.removeEventListener('wheel', onWheel);
		canvas.removeEventListener('contextmenu', onContextMenu);
		canvas.removeEventListener('focus', onFocus);
		canvas.removeEventListener('blur', onBlur);
		window.removeEventListener('mousemove', onMouseMove);
		window.removeEventListener('mouseup', onMouseUp);
	};
}
