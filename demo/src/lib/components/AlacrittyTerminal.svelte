<script lang="ts">
	import { onMount, onDestroy } from 'svelte';

	interface Props {
		wsUrl?: string;
		fontSize?: number;
		fontFamily?: string;
		theme?: 'dark' | 'light';
		class?: string;
		onTerminalReady?: (terminal: any) => void;
	}

	let {
		wsUrl = undefined,
		fontSize = 14,
		fontFamily = "'Fira Code', 'Cascadia Code', 'Source Code Pro', monospace",
		theme = 'dark',
		class: className = '',
		onTerminalReady = undefined
	}: Props = $props();

	let containerEl: HTMLDivElement;
	let canvasEl: HTMLCanvasElement;
	let terminal: any = null;
	let status = $state<'loading' | 'ready' | 'connected' | 'error'>('loading');
	let statusMessage = $state('Initializing...');

	function mapKeyToBytes(e: KeyboardEvent): Uint8Array | null {
		if (e.ctrlKey && e.key.length === 1) {
			const code = e.key.toUpperCase().charCodeAt(0) - 64;
			if (code >= 0 && code <= 31) {
				return new Uint8Array([code]);
			}
		} else if (e.key === 'Enter') {
			return new Uint8Array([13]);
		} else if (e.key === 'Backspace') {
			return new Uint8Array([127]);
		} else if (e.key === 'Tab') {
			return new Uint8Array([9]);
		} else if (e.key === 'Escape') {
			return new Uint8Array([27]);
		} else if (e.key === 'ArrowUp') {
			return new Uint8Array([27, 91, 65]);
		} else if (e.key === 'ArrowDown') {
			return new Uint8Array([27, 91, 66]);
		} else if (e.key === 'ArrowRight') {
			return new Uint8Array([27, 91, 67]);
		} else if (e.key === 'ArrowLeft') {
			return new Uint8Array([27, 91, 68]);
		} else if (e.key === 'Home') {
			return new Uint8Array([27, 91, 72]);
		} else if (e.key === 'End') {
			return new Uint8Array([27, 91, 70]);
		} else if (e.key === 'Delete') {
			return new Uint8Array([27, 91, 51, 126]);
		} else if (e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) {
			return new TextEncoder().encode(e.key);
		} else if (e.altKey && e.key.length === 1) {
			const encoded = new TextEncoder().encode(e.key);
			return new Uint8Array([27, ...encoded]);
		}
		return null;
	}

	function handleKeydown(e: KeyboardEvent) {
		if (!terminal) return;
		const bytes = mapKeyToBytes(e);
		if (bytes) {
			e.preventDefault();
			terminal.write(bytes);
		}
	}

	onMount(async () => {
		try {
			// Dynamic import with URL to prevent Vite from bundling the WASM module.
			const pkgUrl = new URL('/pkg/alacritty_web.js', window.location.origin).href;
			const wasmModule = await import(/* @vite-ignore */ pkgUrl);
			await wasmModule.default();

			terminal = new wasmModule.AlacrittyTerminal(canvasEl);

			status = 'ready';
			statusMessage = 'Terminal ready';

			// Set up resize observer.
			const resizeObserver = new ResizeObserver((entries) => {
				if (!terminal) return;
				const cellW = terminal.cell_width();
				const cellH = terminal.cell_height();
				if (cellW <= 0 || cellH <= 0) return;

				for (const entry of entries) {
					const { width, height } = entry.contentRect;
					const cols = Math.floor(width / cellW);
					const rows = Math.floor(height / cellH);
					if (cols > 0 && rows > 0) {
						terminal.resize(cols, rows);
					}
				}
			});
			resizeObserver.observe(canvasEl);

			// Auto-connect if wsUrl is provided.
			if (wsUrl) {
				try {
					terminal.connect(wsUrl);
					status = 'connected';
					statusMessage = `Connected to ${wsUrl}`;
				} catch (e: any) {
					status = 'error';
					statusMessage = `Connection failed: ${e.message}`;
				}
			}

			canvasEl.addEventListener('keydown', handleKeydown);

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
		if (canvasEl) {
			canvasEl.removeEventListener('keydown', handleKeydown);
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
	<div class="terminal-canvas-container" bind:this={containerEl}>
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
