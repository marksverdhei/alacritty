<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { mapKeyToBytes } from '$lib/key-mapping';
	import { loadAlacrittyConfig, applyAlacrittyConfig, type AlacrittyConfig } from '$lib/alacritty-config';
	import { resolveTheme, themePalette, type Theme } from '$lib/themes';

	interface Props {
		wsUrl?: string;
		fontSize?: number;
		fontFamily?: string;
		theme?: 'dark' | 'light';
		class?: string;
		onTerminalReady?: (terminal: any) => void;
		/**
		 * Optional sink for user input (keystrokes and paste bytes). When set,
		 * the bytes go to this callback instead of the wasm terminal's built-in
		 * write/paste paths. Use this to plug the terminal into something other
		 * than the auto-created WebSocket — e.g. a WebContainer process stdin.
		 * Side effects (scroll-to-bottom, clear-selection) still apply.
		 */
		onInput?: (bytes: Uint8Array) => void;
		/**
		 * Optional pre-parsed alacritty config to apply right after the
		 * terminal mounts — sets the palette and font from the user's
		 * ~/.config/alacritty/alacritty.toml. When undefined (default),
		 * the renderer keeps its built-in defaults.
		 */
		alacrittyConfig?: AlacrittyConfig | null;
		/**
		 * Built-in theme by id ("catppuccin-mocha", "tokyo-night", etc).
		 * Loses to `alacrittyConfig` if both are set.
		 */
		themeName?: string;
		/** Also expose the resolved Theme so chrome wrappers can colour borders. */
		onThemeResolved?: (theme: Theme) => void;
	}

	let {
		wsUrl = undefined,
		fontSize = 14,
		// Default to fonts without contextual ligatures so cell advance is
		// uniform — Fira Code's ligatures make alignment jitter visible.
		fontFamily = "ui-monospace, 'SF Mono', Menlo, Consolas, 'DejaVu Sans Mono', monospace",
		theme = 'dark',
		class: className = '',
		onTerminalReady = undefined,
		onInput = undefined,
		alacrittyConfig = undefined,
		themeName = undefined,
		onThemeResolved = undefined
	}: Props = $props();

	let canvasEl: HTMLCanvasElement;
	let terminal: any = null;
	let status = $state<'loading' | 'ready' | 'connected' | 'error'>('loading');
	let statusMessage = $state('Initializing...');
	let wsPollHandle: number | null = null;
	let resizeObserver: ResizeObserver | null = null;

	// Route input bytes — keystrokes or pasted text — to the right sink.
	// When the parent supplied an `onInput` callback we deliver bytes there and
	// only do the wasm-side side effects locally. Otherwise we fall back to the
	// wasm terminal's own write/paste paths which know how to talk to the
	// internal WebSocket and (for paste) wrap with bracketed-paste markers.
	function deliverInput(bytes: Uint8Array) {
		if (onInput) {
			terminal.scroll_to_bottom?.();
			terminal.selection_clear?.();
			onInput(bytes);
		} else {
			terminal.write(bytes);
		}
	}

	function deliverPaste(text: string) {
		if (onInput) {
			terminal.scroll_to_bottom?.();
			terminal.selection_clear?.();
			onInput(new TextEncoder().encode(text));
		} else {
			terminal.paste(text);
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (!terminal) return;
		// Ctrl/Cmd+Shift+C: copy the active selection.
		if ((e.ctrlKey || e.metaKey) && e.shiftKey && (e.key === 'C' || e.key === 'c')) {
			const text = terminal.selection_text?.();
			if (text) {
				navigator.clipboard?.writeText(text).catch(() => {});
				e.preventDefault();
				return;
			}
		}
		// Ctrl/Cmd+Shift+V (and Cmd+V on mac): paste from clipboard.
		const isPaste =
			((e.ctrlKey || e.metaKey) && e.shiftKey && (e.key === 'V' || e.key === 'v')) ||
			(e.metaKey && !e.shiftKey && (e.key === 'V' || e.key === 'v'));
		if (isPaste) {
			e.preventDefault();
			navigator.clipboard?.readText().then((text) => {
				if (text) deliverPaste(text);
			}).catch(() => {});
			return;
		}
		const bytes = mapKeyToBytes(e);
		if (bytes) {
			e.preventDefault();
			deliverInput(bytes);
		}
	}

	// Pack DOM modifier keys into the bit format `report_mouse` expects.
	function modBits(e: MouseEvent | WheelEvent): number {
		return (e.shiftKey ? 1 : 0) | ((e.altKey || e.metaKey) ? 2 : 0) | (e.ctrlKey ? 4 : 0);
	}

	function handleWheel(e: WheelEvent) {
		if (!terminal) return;
		// When an app like htop/vim has mouse reporting on, wheel events
		// become button events (64 = wheel-up, 65 = wheel-down). Skip the
		// scrollback adjustment in that case — let the app handle it.
		if (terminal.mouse_reporting_active?.()) {
			const cell = cellFromEvent(e as unknown as MouseEvent);
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
		// Normalize deltaY into line-count. Chrome reports pixels, Firefox
		// reports lines via deltaMode=1. Clamp to keep single wheel ticks sensible.
		const lineHeight = terminal.cell_height?.() || 16;
		let lines: number;
		if (e.deltaMode === 1) {
			lines = e.deltaY;
		} else {
			lines = e.deltaY / lineHeight;
		}
		// Positive deltaY = scroll DOWN in the document, which in terminal
		// scrollback means moving towards newer output (negative delta).
		const delta = -Math.round(lines * 3);
		if (delta === 0) return;
		e.preventDefault();
		terminal.scroll(delta);
	}

	// Selection state (JS-side mirror — actual cells live in the wasm Term).
	let dragging = false;
	// Which button is held during mouse-reported drags; `null` when no
	// button is pressed. Drives the motion-event encoding (button+32).
	let reportedButton: number | null = null;

	function cellFromEvent(e: MouseEvent): { row: number; col: number; sideLeft: boolean } | null {
		if (!terminal) return null;
		const cellW = terminal.cell_width();
		const cellH = terminal.cell_height();
		if (cellW <= 0 || cellH <= 0) return null;
		const rect = canvasEl.getBoundingClientRect();
		const x = e.clientX - rect.left;
		const y = e.clientY - rect.top;
		const col = Math.max(0, Math.min(terminal.cols() - 1, Math.floor(x / cellW)));
		const row = Math.max(0, Math.min(terminal.rows() - 1, Math.floor(y / cellH)));
		const inCellX = x - col * cellW;
		return { row, col, sideLeft: inCellX < cellW / 2 };
	}

	function handleMouseDown(e: MouseEvent) {
		if (!terminal) return;
		const cell = cellFromEvent(e);
		if (!cell) return;
		// Mouse reporting takes priority: when an app like vim/htop has
		// DECSET 1000/1002/1003 on, the mouse drives the app, not the
		// browser-side selection. Shift acts as the standard "force browser
		// selection" override that xterm-family terminals use.
		if (terminal.mouse_reporting_active?.() && !e.shiftKey) {
			const button = e.button === 1 ? 1 : e.button === 2 ? 2 : 0;
			const bytes = terminal.report_mouse(button, 0, cell.col, cell.row, modBits(e));
			if (bytes) {
				e.preventDefault();
				deliverInput(bytes);
				reportedButton = button;
				canvasEl.focus();
				return;
			}
		}
		if (e.button !== 0) return; // left-click only for selection
		// detail: 1 = single click, 2 = double, 3 = triple — matches native terms.
		if (e.detail >= 3) {
			terminal.selection_line(cell.row, cell.col);
			// Auto-copy on triple-click.
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
		canvasEl.focus();
	}

	function handleMouseMove(e: MouseEvent) {
		if (!terminal) return;
		// Mouse-reporting path: send motion events when the active mode wants
		// them. `report_mouse` itself rejects motion events the mode hasn't
		// asked for, so we always send and just discard the empty result.
		if (terminal.mouse_reporting_active?.()) {
			const cell = cellFromEvent(e);
			if (!cell) return;
			// Button 3 = "none held" in xterm encoding. For drag mode the
			// rust side filters that out automatically.
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

	function handleMouseUp(e: MouseEvent) {
		if (!terminal) return;
		if (reportedButton !== null) {
			const cell = cellFromEvent(e);
			if (cell) {
				const bytes = terminal.report_mouse(
					reportedButton,
					1,
					cell.col,
					cell.row,
					modBits(e),
				);
				if (bytes) deliverInput(bytes);
			}
			reportedButton = null;
			return;
		}
		if (!dragging) return;
		dragging = false;
		// Copy the selection to the clipboard if non-empty. This mirrors
		// Alacritty's default "copy on selection" behaviour.
		const text = terminal.selection_text();
		if (text) {
			navigator.clipboard?.writeText(text).catch(() => { /* permission denied */ });
		}
	}

	// Named so onDestroy can remove them — anonymous arrows would leak.
	function handleFocus() { terminal?.set_focused(true); }
	function handleBlur() { terminal?.set_focused(false); }

	// Suppress the browser context menu when mouse-reporting is on so right
	// clicks reach the running app (e.g. `htop` menus). Shift-right-click is
	// the standard escape to get the browser menu back.
	function handleContextMenu(e: MouseEvent) {
		if (!terminal) return;
		if (terminal.mouse_reporting_active?.() && !e.shiftKey) {
			e.preventDefault();
		}
	}

	onMount(async () => {
		try {
			// Dynamic import with URL to prevent Vite from bundling the WASM module.
			const pkgUrl = new URL('/pkg/alacritty_web.js', window.location.origin).href;
			const wasmModule = await import(/* @vite-ignore */ pkgUrl);
			// CRITICAL: Only call default() once across ALL component instances.
			// Each call re-instantiates the WASM module and rewires the shared
			// `wasm` variable in the JS glue, so subsequent method calls route
			// to the most recent instance — not the one you think you're calling.
			const w = window as any;
			if (!w.__alacrittyWasmInit) {
				w.__alacrittyWasmInit = wasmModule.default();
			}
			await w.__alacrittyWasmInit;

			terminal = new wasmModule.AlacrittyTerminal(canvasEl);
			// Apply caller-supplied font props now that the instance exists.
			// Order matters: set family first, then size, since the wasm side
			// remeasures cell metrics on each call and the size depends on
			// the family's glyph advance.
			try {
				terminal.set_font_family(fontFamily);
				terminal.set_font_size(fontSize);
			} catch (e) {
				console.warn('Could not apply font props:', e);
			}
			// Pick the source of the palette: explicit alacrittyConfig wins,
			// otherwise fall back to the named built-in theme.
			if (alacrittyConfig) {
				try {
					applyAlacrittyConfig(terminal, alacrittyConfig);
				} catch (e) {
					console.warn('Could not apply alacritty config:', e);
				}
			} else if (themeName) {
				try {
					const t = resolveTheme(themeName);
					terminal.set_palette(themePalette(t));
					onThemeResolved?.(t);
				} catch (e) {
					console.warn('Could not apply theme:', e);
				}
			}

			status = 'ready';
			statusMessage = 'Terminal ready';

			// Sync the canvas backing store to its CSS size, then wire
			// up a ResizeObserver to keep it in sync and tell the
			// terminal about grid resizes. We wait for the next RAF
			// before doing any sizing so we don't fight with the very
			// first render frame.
			const applySize = () => {
				if (!terminal) return;
				terminal.sync_canvas_size();
				const cellW = terminal.cell_width();
				const cellH = terminal.cell_height();
				if (cellW <= 0 || cellH <= 0) return;
				const w = canvasEl.clientWidth;
				const h = canvasEl.clientHeight;
				const cols = Math.max(1, Math.floor(w / cellW));
				const rows = Math.max(1, Math.floor(h / cellH));
				terminal.resize(cols, rows);
			};
			requestAnimationFrame(applySize);
			resizeObserver = new ResizeObserver(() => applySize());
			resizeObserver.observe(canvasEl);

			// Auto-connect if wsUrl is provided. The wasm constructor of the
			// WebSocket is synchronous but the actual handshake is async, so we
			// poll the ready state to keep the visible status honest instead of
			// flipping to "connected" before the socket has actually opened.
			if (wsUrl) {
				try {
					terminal.connect(wsUrl);
					status = 'loading';
					statusMessage = `Connecting to ${wsUrl}…`;
					wsPollHandle = window.setInterval(() => {
						if (!terminal) return;
						const rs = terminal.ws_ready_state();
						if (rs === 1 && status !== 'connected') {
							status = 'connected';
							statusMessage = `Connected to ${wsUrl}`;
						} else if ((rs === 2 || rs === 3) && status !== 'error') {
							status = 'error';
							statusMessage = `Disconnected from ${wsUrl}`;
						}
					}, 250);
				} catch (e: any) {
					status = 'error';
					statusMessage = `Connection failed: ${e.message}`;
				}
			}

			canvasEl.addEventListener('keydown', handleKeydown);
			canvasEl.addEventListener('wheel', handleWheel, { passive: false });
			canvasEl.addEventListener('focus', handleFocus);
			canvasEl.addEventListener('blur', handleBlur);
			canvasEl.addEventListener('mousedown', handleMouseDown);
			canvasEl.addEventListener('contextmenu', handleContextMenu);
			// Mouse move/up listen on window so dragging off-canvas still works.
			window.addEventListener('mousemove', handleMouseMove);
			window.addEventListener('mouseup', handleMouseUp);
			// Seed the initial state from whatever the DOM says — if the
			// canvas is already the active element, we want the filled cursor.
			terminal.set_focused(document.activeElement === canvasEl);

			if (onTerminalReady) {
				onTerminalReady(terminal);
			}
		} catch (e: any) {
			status = 'error';
			statusMessage = `Init failed: ${e.message}`;
			console.error('Terminal init failed:', e);
		}
	});

	onDestroy(() => {
		if (wsPollHandle !== null) {
			clearInterval(wsPollHandle);
			wsPollHandle = null;
		}
		if (resizeObserver) {
			resizeObserver.disconnect();
			resizeObserver = null;
		}
		if (canvasEl) {
			canvasEl.removeEventListener('keydown', handleKeydown);
			canvasEl.removeEventListener('wheel', handleWheel);
			canvasEl.removeEventListener('mousedown', handleMouseDown);
			canvasEl.removeEventListener('contextmenu', handleContextMenu);
			canvasEl.removeEventListener('focus', handleFocus);
			canvasEl.removeEventListener('blur', handleBlur);
		}
		window.removeEventListener('mousemove', handleMouseMove);
		window.removeEventListener('mouseup', handleMouseUp);
		if (terminal) {
			try {
				terminal.dispose();
			} catch {
				// Already disposed.
			}
			terminal = null;
		}
	});
</script>

<div class="terminal-wrapper {className}" class:theme-light={theme === 'light'}>
	<div class="terminal-status">
		<span class="status-dot" class:connected={status === 'connected' || status === 'ready'}
			class:error={status === 'error'}></span>
		<span class="status-text">{statusMessage}</span>
	</div>
	<div class="terminal-canvas-container">
		<canvas
			bind:this={canvasEl}
			tabindex="0"
			class="terminal-canvas"
		></canvas>
	</div>
</div>

<style>
	.terminal-wrapper {
		display: flex;
		flex-direction: column;
		background: #1d1f21;
		border: 1px solid #373b41;
		border-radius: 8px;
		overflow: hidden;
	}

	.terminal-wrapper.theme-light {
		background: #fafafa;
		border-color: #d0d0d0;
	}

	.terminal-status {
		display: flex;
		align-items: center;
		padding: 6px 12px;
		background: #282a2e;
		border-bottom: 1px solid #373b41;
		font-size: 12px;
		gap: 8px;
	}

	.theme-light .terminal-status {
		background: #e8e8e8;
		border-color: #d0d0d0;
	}

	.status-dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: #969896;
		flex-shrink: 0;
	}

	.status-dot.connected {
		background: #b5bd68;
	}

	.status-dot.error {
		background: #cc6666;
	}

	.status-text {
		color: #969896;
		font-family: system-ui, -apple-system, sans-serif;
	}

	.terminal-canvas-container {
		flex: 1;
		min-height: 300px;
		position: relative;
	}

	.terminal-canvas {
		width: 100%;
		height: 100%;
		display: block;
		outline: none;
	}
</style>
