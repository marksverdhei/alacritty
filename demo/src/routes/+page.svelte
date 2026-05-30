<script lang="ts">
	import { onMount } from 'svelte';
	import AlacrittyTerminal from '$lib/components/AlacrittyTerminal.svelte';
	import { replayFrames } from '$lib/replay-data';
	import { loadAlacrittyConfig, type AlacrittyConfig } from '$lib/alacritty-config';

	let replayTerminal: any = null;
	let replayRunning = $state(false);

	let webcontainerStatus = $state('Not started');

	// Pull the user's ~/.config/alacritty/alacritty.toml (copied into static/
	// at build time) so all three cards render with their actual theme + font.
	let alacrittyConfig = $state<AlacrittyConfig | null>(null);
	let configLabel = $state('loading config…');
	onMount(async () => {
		const cfg = await loadAlacrittyConfig();
		if (cfg) {
			alacrittyConfig = cfg;
			configLabel = cfg.fontFamily
				? `Using ${cfg.fontFamily} @ ${cfg.fontSize ?? '14'}pt + theme palette`
				: 'Using your config palette';
		} else {
			configLabel = 'No alacritty.toml found — using defaults';
		}
	});

	// Replay logic: feed pre-recorded frames into the terminal.
	async function startReplay(terminal: any) {
		replayTerminal = terminal;
		await runReplay();
	}

	async function runReplay() {
		if (!replayTerminal || replayRunning) return;
		replayRunning = true;

		const encoder = new TextEncoder();
		// Capture the specific instance — don't follow later reassignments of
		// replayTerminal during the loop's async waits.
		const target = replayTerminal;
		for (const [delay, data] of replayFrames) {
			if (!target) break;
			await new Promise((r) => setTimeout(r, delay));
			target.feed(encoder.encode(data));
		}

		replayRunning = false;
	}

	// WebContainer logic: boot a VM and spawn a shell. The terminal is the
	// AlacrittyTerminal instance — we set `webcontainerInput` so the component's
	// keydown/paste handlers feed bytes to us, then we forward them to the
	// shell's stdin. This way the WebContainer card still gets scrollback,
	// selection, paste, Ctrl+Shift+C — everything the other cards have.
	const decoder = new TextDecoder();
	let webcontainerInput: ((bytes: Uint8Array) => void) | null = $state(null);

	async function initWebContainer(terminal: any) {
		webcontainerStatus = 'Booting WebContainer...';

		try {
			const { WebContainer } = await import('@webcontainer/api');
			const wc = await WebContainer.boot();
			webcontainerStatus = 'Spawning shell...';

			const proc = await wc.spawn('jsh');
			webcontainerStatus = 'Shell running';

			// One writer for the lifetime of the process, not one per keystroke.
			const writer = proc.input.getWriter();
			webcontainerInput = (bytes: Uint8Array) => {
				writer.write(decoder.decode(bytes)).catch((err) => {
					console.error('WebContainer write failed:', err);
				});
			};

			// Pipe shell output to terminal.
			const reader = proc.output.getReader();
			const encoder = new TextEncoder();
			(async () => {
				try {
					while (true) {
						const { done, value } = await reader.read();
						if (done) break;
						if (typeof value === 'string') {
							terminal.feed(encoder.encode(value));
						} else {
							terminal.feed(value);
						}
					}
					webcontainerStatus = 'Shell exited';
				} catch (err: any) {
					webcontainerStatus = `Output error: ${err.message ?? err}`;
				}
			})();
		} catch (e: any) {
			webcontainerStatus = `Failed: ${e.message}`;
			console.error('WebContainer init failed:', e);
			const encoder = new TextEncoder();
			terminal.feed(encoder.encode(
				`\x1b[31mWebContainer initialization failed.\x1b[0m\r\n` +
				`\x1b[33mThis feature requires specific HTTP headers (COOP/COEP)\x1b[0m\r\n` +
				`\x1b[33mand may not work in all environments.\x1b[0m\r\n\r\n` +
				`\x1b[2mError: ${e.message}\x1b[0m\r\n`
			));
		}
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
		<p class="cta">
			<a href="/compare">Side-by-side vs xterm.js →</a>
			<span class="cta-sep">·</span>
			<a href="/library">Component library →</a>
		</p>
		<p class="config-line">{configLabel}</p>
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
					{alacrittyConfig}
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
					{alacrittyConfig}
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
					{alacrittyConfig}
					onTerminalReady={initWebContainer}
					onInput={webcontainerInput ?? undefined}
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

	.cta {
		margin-top: 1rem;
		font-size: 0.95rem;
	}
	.cta a {
		color: #81a2be;
		text-decoration: none;
		border-bottom: 1px dashed #81a2be;
		padding-bottom: 2px;
	}
	.cta a:hover {
		color: #b5bd68;
		border-bottom-color: #b5bd68;
	}
	.cta-sep {
		color: #4a4f57;
		margin: 0 0.5rem;
	}

	.config-line {
		margin-top: 0.6rem;
		font-size: 0.78rem;
		color: #6f7782;
		font-family: ui-monospace, Menlo, monospace;
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
