<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { mapKeyToBytes } from '$lib/key-mapping';
	import { wireTerminalCanvas } from '$lib/canvas-handlers';
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
		/**
		 * Optional callback fired whenever the shell pushes a new OSC 0/2
		 * window title and the new value differs from the previous one. The
		 * component polls at ~4 Hz; the parent decides whether to mirror it
		 * into `document.title`, status chrome, etc.
		 */
		onTitle?: (title: string) => void;
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
		onThemeResolved = undefined,
		onTitle = undefined
	}: Props = $props();

	let canvasEl: HTMLCanvasElement;
	let terminal: any = null;
	let status = $state<'loading' | 'ready' | 'connected' | 'error'>('loading');
	let statusMessage = $state('Initializing...');
	let wsPollHandle: number | null = null;
	let titlePollHandle: number | null = null;
	let lastTitle: string | undefined = undefined;
	let resizeObserver: ResizeObserver | null = null;
	let unwireCanvas: (() => void) | null = null;

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
		const modes = terminal.keyboard_mode_bits?.() ?? 0;
		const bytes = mapKeyToBytes(e, modes);
		if (bytes) {
			e.preventDefault();
			deliverInput(bytes);
		}
	}

	// Mouse, wheel, focus, contextmenu handlers all live in the shared
	// wireTerminalCanvas helper — see $lib/canvas-handlers.ts. Keyboard
	// (handleKeydown) stays here because the paste path is component-specific.

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
			// Wire the rest — mouse, wheel, focus/blur, contextmenu — via the
			// shared helper. deliverInput handles the onInput vs internal-WS
			// routing for us.
			unwireCanvas = wireTerminalCanvas(canvasEl, terminal, {
				sendInput: deliverInput,
			});
			// Seed the initial state from whatever the DOM says — if the
			// canvas is already the active element, we want the filled cursor.
			terminal.set_focused(document.activeElement === canvasEl);

			if (onTerminalReady) {
				onTerminalReady(terminal);
			}

			// Poll the OSC 0/2 title at 4 Hz and fire onTitle on change. Polling
			// (vs. an FFI callback) keeps the JS↔WASM boundary trivial — no
			// closure ownership to track.
			if (onTitle) {
				titlePollHandle = window.setInterval(() => {
					if (!terminal) return;
					const t = terminal.title?.();
					if (t && t !== lastTitle) {
						lastTitle = t;
						onTitle(t);
					}
				}, 250);
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
		if (titlePollHandle !== null) {
			clearInterval(titlePollHandle);
			titlePollHandle = null;
		}
		if (resizeObserver) {
			resizeObserver.disconnect();
			resizeObserver = null;
		}
		if (canvasEl) {
			canvasEl.removeEventListener('keydown', handleKeydown);
		}
		if (unwireCanvas) {
			unwireCanvas();
			unwireCanvas = null;
		}
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
