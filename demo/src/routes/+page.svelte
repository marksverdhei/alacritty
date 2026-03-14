<script lang="ts">
	import { onMount } from 'svelte';
	import AlacrittyTerminal from '$lib/components/AlacrittyTerminal.svelte';
	import { replayFrames } from '$lib/replay-data';

	let replayTerminal: any = null;
	let replayRunning = $state(false);

	let webcontainerTerminal: any = null;
	let webcontainerStatus = $state('Not started');
	let webcontainerSupported = $state(true);

	// Replay logic: feed pre-recorded frames into the terminal.
	async function startReplay(terminal: any) {
		replayTerminal = terminal;
		await runReplay();
	}

	async function runReplay() {
		if (!replayTerminal || replayRunning) return;
		replayRunning = true;

		const encoder = new TextEncoder();
		for (const [delay, data] of replayFrames) {
			if (!replayTerminal) break;
			await new Promise((r) => setTimeout(r, delay));
			replayTerminal.feed(encoder.encode(data));
		}

		replayRunning = false;
	}

	// WebContainer logic: boot a VM and spawn a shell.
	async function initWebContainer(terminal: any) {
		webcontainerTerminal = terminal;
		webcontainerStatus = 'Booting WebContainer...';

		try {
			const { WebContainer } = await import('@webcontainer/api');
			const wc = await WebContainer.boot();
			webcontainerStatus = 'Spawning shell...';

			const proc = await wc.spawn('jsh');
			webcontainerStatus = 'Shell running';

			// Pipe shell output to terminal.
			const reader = proc.output.getReader();
			const encoder = new TextEncoder();
			(async () => {
				while (true) {
					const { done, value } = await reader.read();
					if (done) break;
					// value is a string from WebContainers
					if (typeof value === 'string') {
						terminal.feed(encoder.encode(value));
					} else {
						terminal.feed(value);
					}
				}
			})();

			// Pipe terminal keyboard input to shell stdin.
			// We override the terminal's keydown handler to write to the process.
			const canvas = terminal.canvas || document.querySelector('.webcontainer-terminal canvas');
			if (canvas) {
				canvas.addEventListener('keydown', (e: KeyboardEvent) => {
					const bytes = mapKeyToBytes(e);
					if (bytes) {
						e.preventDefault();
						e.stopPropagation();
						const writer = proc.input.getWriter();
						writer.write(new TextDecoder().decode(bytes));
						writer.releaseLock();
					}
				}, { capture: true });
			}
		} catch (e: any) {
			webcontainerStatus = `Failed: ${e.message}`;
			webcontainerSupported = false;
			console.error('WebContainer init failed:', e);
			// Show error in terminal
			const encoder = new TextEncoder();
			terminal.feed(encoder.encode(
				`\x1b[31mWebContainer initialization failed.\x1b[0m\r\n` +
				`\x1b[33mThis feature requires specific HTTP headers (COOP/COEP)\x1b[0m\r\n` +
				`\x1b[33mand may not work in all environments.\x1b[0m\r\n\r\n` +
				`\x1b[2mError: ${e.message}\x1b[0m\r\n`
			));
		}
	}

	function mapKeyToBytes(e: KeyboardEvent): Uint8Array | null {
		if (e.ctrlKey && e.key.length === 1) {
			const code = e.key.toUpperCase().charCodeAt(0) - 64;
			if (code >= 0 && code <= 31) return new Uint8Array([code]);
		} else if (e.key === 'Enter') return new Uint8Array([13]);
		else if (e.key === 'Backspace') return new Uint8Array([127]);
		else if (e.key === 'Tab') return new Uint8Array([9]);
		else if (e.key === 'Escape') return new Uint8Array([27]);
		else if (e.key === 'ArrowUp') return new Uint8Array([27, 91, 65]);
		else if (e.key === 'ArrowDown') return new Uint8Array([27, 91, 66]);
		else if (e.key === 'ArrowRight') return new Uint8Array([27, 91, 67]);
		else if (e.key === 'ArrowLeft') return new Uint8Array([27, 91, 68]);
		else if (e.key === 'Home') return new Uint8Array([27, 91, 72]);
		else if (e.key === 'End') return new Uint8Array([27, 91, 70]);
		else if (e.key === 'Delete') return new Uint8Array([27, 91, 51, 126]);
		else if (e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey)
			return new TextEncoder().encode(e.key);
		else if (e.altKey && e.key.length === 1)
			return new Uint8Array([27, ...new TextEncoder().encode(e.key)]);
		return null;
	}
</script>

<div class="page">
	<!-- Hero Section -->
	<header class="hero">
		<h1>Alacritty <span class="accent">Web</span></h1>
		<p class="subtitle">GPU-accelerated terminal in your browser</p>
		<p class="description">
			The fastest terminal emulator, compiled to WebAssembly.
			Full VT100/xterm emulation, ANSI colors, and Canvas2D rendering —
			all running client-side.
		</p>
		<div class="badges">
			<span class="badge">Rust + WASM</span>
			<span class="badge">Canvas2D</span>
			<span class="badge">WebSocket PTY</span>
			<span class="badge">MIT/Apache-2.0</span>
		</div>
	</header>

	<!-- Terminal Cards -->
	<section class="terminals">
		<!-- Demo/Replay Terminal -->
		<div class="terminal-card">
			<div class="card-header">
				<h2>Demo Terminal</h2>
				<p class="card-description">
					Pre-recorded session showcasing colors, formatting, and terminal capabilities.
					No server connection needed.
				</p>
				<button class="replay-btn" onclick={() => runReplay()} disabled={replayRunning}>
					{replayRunning ? 'Playing...' : 'Replay'}
				</button>
			</div>
			<div class="card-body">
				<AlacrittyTerminal
					fontSize={14}
					theme="dark"
					onTerminalReady={startReplay}
				/>
			</div>
		</div>

		<!-- WebSocket Terminal -->
		<div class="terminal-card">
			<div class="card-header">
				<h2>WebSocket Terminal</h2>
				<p class="card-description">
					Connects to a local PTY server via WebSocket.
					Start the server with <code>cargo run -p alacritty_pty_server</code>,
					then click the terminal and type.
				</p>
			</div>
			<div class="card-body">
				<AlacrittyTerminal
					wsUrl="ws://localhost:7681"
					fontSize={14}
					theme="dark"
				/>
			</div>
		</div>

		<!-- WebContainer Terminal -->
		<div class="terminal-card">
			<div class="card-header">
				<h2>WebContainer Terminal</h2>
				<p class="card-description">
					Runs a Node.js shell entirely in your browser using WebContainers.
					No server needed — the VM runs client-side.
				</p>
				<span class="wc-status">{webcontainerStatus}</span>
			</div>
			<div class="card-body webcontainer-terminal">
				<AlacrittyTerminal
					fontSize={14}
					theme="dark"
					onTerminalReady={initWebContainer}
				/>
			</div>
		</div>
	</section>

	<!-- Footer -->
	<footer class="footer">
		<p>
			<a href="https://github.com/marksverdhei/alacritty" target="_blank" rel="noopener">
				GitHub
			</a>
			&middot;
			Built with Alacritty + Rust + wasm-bindgen + SvelteKit
		</p>
	</footer>
</div>

<style>
	.page {
		max-width: 1200px;
		margin: 0 auto;
		padding: 2rem 1.5rem;
	}

	/* Hero */
	.hero {
		text-align: center;
		padding: 3rem 0 2rem;
	}

	.hero h1 {
		font-size: 3rem;
		font-weight: 700;
		color: #e0e0e0;
		margin-bottom: 0.5rem;
	}

	.accent {
		color: #81a2be;
	}

	.subtitle {
		font-size: 1.25rem;
		color: #969896;
		margin-bottom: 1rem;
	}

	.description {
		max-width: 600px;
		margin: 0 auto 1.5rem;
		color: #808080;
		line-height: 1.6;
	}

	.badges {
		display: flex;
		justify-content: center;
		gap: 0.5rem;
		flex-wrap: wrap;
	}

	.badge {
		background: #282a2e;
		border: 1px solid #373b41;
		color: #969896;
		padding: 4px 12px;
		border-radius: 12px;
		font-size: 0.8rem;
	}

	/* Terminal Cards */
	.terminals {
		display: flex;
		flex-direction: column;
		gap: 2rem;
		margin-top: 2rem;
	}

	.terminal-card {
		background: #1a1c20;
		border: 1px solid #2a2d33;
		border-radius: 12px;
		overflow: hidden;
	}

	.card-header {
		padding: 1.25rem 1.5rem;
		border-bottom: 1px solid #2a2d33;
	}

	.card-header h2 {
		font-size: 1.1rem;
		font-weight: 600;
		color: #c5c8c6;
		margin-bottom: 0.4rem;
	}

	.card-description {
		font-size: 0.85rem;
		color: #707070;
		line-height: 1.5;
	}

	.card-description code {
		background: #282a2e;
		padding: 2px 6px;
		border-radius: 4px;
		font-size: 0.8rem;
		color: #b5bd68;
	}

	.card-body {
		height: 400px;
	}

	.replay-btn {
		margin-top: 0.75rem;
		background: #373b41;
		color: #c5c8c6;
		border: 1px solid #4d5057;
		padding: 6px 16px;
		border-radius: 6px;
		cursor: pointer;
		font-size: 0.85rem;
		transition: background 0.2s;
	}

	.replay-btn:hover:not(:disabled) {
		background: #4d5057;
	}

	.replay-btn:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	.wc-status {
		display: inline-block;
		margin-top: 0.5rem;
		font-size: 0.8rem;
		color: #969896;
	}

	/* Footer */
	.footer {
		text-align: center;
		padding: 2rem 0;
		margin-top: 2rem;
		border-top: 1px solid #2a2d33;
		color: #606060;
		font-size: 0.85rem;
	}

	/* Responsive */
	@media (max-width: 768px) {
		.hero h1 {
			font-size: 2rem;
		}

		.card-body {
			height: 300px;
		}

		.page {
			padding: 1rem;
		}
	}
</style>
