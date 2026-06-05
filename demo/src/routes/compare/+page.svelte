<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import '@xterm/xterm/css/xterm.css';
	import { mapKeyToBytes } from '$lib/key-mapping';
	import { wireTerminalCanvas } from '$lib/canvas-handlers';
	import { loadAlacrittyConfig, applyAlacrittyConfig, type AlacrittyConfig } from '$lib/alacritty-config';

	// Same PTY stream fed into xterm.js on the left and alacritty-wasm on the right.
	// One WebSocket; keystrokes from either pane go to the same shell.

	const WS_URL = 'ws://localhost:7681';
	const MSG_DATA = 0x00;
	const MSG_RESIZE = 0x01;

	let xtermEl: HTMLDivElement;
	let alacrittyCanvas: HTMLCanvasElement;

	let xtermTerm: any = null;
	let xtermFit: any = null;
	let alacritty: any = null;
	let ws: WebSocket | null = null;
	let status = $state<'connecting' | 'open' | 'closed' | 'error'>('connecting');
	let bytesIn = $state(0);
	let bytesOut = $state(0);
	let xtermRenderMs = $state(0);
	let alacrittyRenderMs = $state(0);

	// User's alacritty config (loaded once on mount).
	let userConfig: AlacrittyConfig | null = $state(null);
	let userConfigActive = $state(false);

	function toggleUserConfig() {
		if (!alacritty || !xtermTerm || !userConfig) return;
		userConfigActive = !userConfigActive;
		if (userConfigActive) {
			// Apply to alacritty via wasm.
			applyAlacrittyConfig(alacritty, userConfig);
			// Apply matching theme + font to xterm.js so the comparison stays fair.
			const p = userConfig.palette;
			const xtermTheme: Record<string, string> = {};
			if (p.background) xtermTheme.background = p.background;
			if (p.foreground) xtermTheme.foreground = p.foreground;
			if (p.cursor) xtermTheme.cursor = p.cursor;
			if (p.black) xtermTheme.black = p.black;
			if (p.red) xtermTheme.red = p.red;
			if (p.green) xtermTheme.green = p.green;
			if (p.yellow) xtermTheme.yellow = p.yellow;
			if (p.blue) xtermTheme.blue = p.blue;
			if (p.magenta) xtermTheme.magenta = p.magenta;
			if (p.cyan) xtermTheme.cyan = p.cyan;
			if (p.white) xtermTheme.white = p.white;
			if (p.bright_black) xtermTheme.brightBlack = p.bright_black;
			if (p.bright_red) xtermTheme.brightRed = p.bright_red;
			if (p.bright_green) xtermTheme.brightGreen = p.bright_green;
			if (p.bright_yellow) xtermTheme.brightYellow = p.bright_yellow;
			if (p.bright_blue) xtermTheme.brightBlue = p.bright_blue;
			if (p.bright_magenta) xtermTheme.brightMagenta = p.bright_magenta;
			if (p.bright_cyan) xtermTheme.brightCyan = p.bright_cyan;
			if (p.bright_white) xtermTheme.brightWhite = p.bright_white;
			try { xtermTerm.options.theme = xtermTheme; } catch {}
			if (userConfig.fontFamily) {
				try {
					xtermTerm.options.fontFamily =
						`${/[ '"]/.test(userConfig.fontFamily) ? `'${userConfig.fontFamily}'` : userConfig.fontFamily}, ui-monospace, Menlo, monospace`;
				} catch {}
			}
			if (userConfig.fontSize && userConfig.fontSize > 0) {
				try { xtermTerm.options.fontSize = userConfig.fontSize * 1.333; } catch {}
			}
		}
	}

	// Stress benchmark state. The numbers are populated by runStress().
	let stressRunning = $state(false);
	let stressProgress = $state('');
	let stressResult = $state<null | {
		bytes: number;
		trials: number;
		xterm: { median: number; min: number; max: number; mbps: number };
		alacritty: {
			median: number;
			min: number;
			max: number;
			mbps: number;
			parseMedian: number;
			renderMedian: number;
		};
		ratio: number;
	}>(null);
	// Handlers + disposables hoisted to module scope so onDestroy can clean
	// up properly. Each is initialised inside onMount once the wasm + xterm
	// instances exist.
	let unwireCanvas: (() => void) | null = null;
	let xtermScrollSub: { dispose(): void } | null = null;
	let resizeObserver: ResizeObserver | null = null;

	/**
	 * Build a ~200KB chunk of richly-formatted terminal output: random words
	 * separated by random ANSI colour escapes (FG, BG, bold, underline,
	 * 256-colour, truecolor). Mixed so it exercises every path of the
	 * renderer — bg fills, text runs, run breaks on style change, underline
	 * lines. Deterministic so repeated runs are comparable.
	 */
	function buildStressPayload(targetBytes: number): Uint8Array {
		const ESC = '\x1b';
		const CSI = ESC + '[';
		const words = [
			'alacritty', 'wasm', 'canvas2d', 'xterm', 'render', 'fillText',
			'monospace', 'glyph', 'cell', 'grid', 'unicode', 'compose',
			'paint', 'frame', 'metric', 'buffer', 'history', 'scroll',
		];
		let seed = 1;
		const rand = () => {
			// xorshift32 — deterministic, no Math.random.
			seed ^= seed << 13;
			seed ^= seed >>> 17;
			seed ^= seed << 5;
			return (seed >>> 0) / 0xffffffff;
		};
		let out = '';
		while (out.length < targetBytes) {
			const sgr: string[] = [];
			// 40% chance of an ANSI fg/bg, 20% of 256-colour, 10% truecolor.
			const r = rand();
			if (r < 0.4) sgr.push(`3${Math.floor(rand() * 8)}`);
			else if (r < 0.6) sgr.push(`38;5;${16 + Math.floor(rand() * 200)}`);
			else if (r < 0.7) sgr.push(`38;2;${Math.floor(rand() * 256)};${Math.floor(rand() * 256)};${Math.floor(rand() * 256)}`);
			const r2 = rand();
			if (r2 < 0.3) sgr.push(`4${Math.floor(rand() * 8)}`);
			else if (r2 < 0.45) sgr.push(`48;5;${16 + Math.floor(rand() * 200)}`);
			if (rand() < 0.25) sgr.push('1'); // bold
			if (rand() < 0.15) sgr.push('4'); // underline
			const prefix = sgr.length ? `${CSI}${sgr.join(';')}m` : '';
			const w = words[Math.floor(rand() * words.length)];
			out += `${prefix}${w}${CSI}0m `;
			if (rand() < 0.06) out += '\r\n';
		}
		return new TextEncoder().encode(out);
	}

	// Wait until xterm has fully processed every queued chunk. The write
	// overload takes a callback that fires when the parser drains.
	function xtermFlush(): Promise<void> {
		return new Promise((resolve) => {
			xtermTerm.write('', () => resolve());
		});
	}

	function nextFrame(): Promise<void> {
		return new Promise((r) => requestAnimationFrame(() => r()));
	}

	/**
	 * Wait until the wasm render loop has advanced `frame_seq` AND drained
	 * everything we fed. Polls once per RAF (the wasm loop's own cadence)
	 * with a safety timeout so we don't hang on a stalled tab.
	 */
	async function waitForAlacrittyRender(seqBefore: number, timeoutMs = 5000): Promise<boolean> {
		const start = performance.now();
		while (alacritty.frame_seq() === seqBefore || alacritty.pending_chunks() > 0) {
			await nextFrame();
			if (performance.now() - start > timeoutMs) return false;
		}
		return true;
	}

	function median(values: number[]): number {
		const s = [...values].sort((a, b) => a - b);
		const m = Math.floor(s.length / 2);
		return s.length % 2 === 0 ? (s[m - 1] + s[m]) / 2 : s[m];
	}

	type Trial = { xtermMs: number; alacrittyMs: number; parseMs: number; renderMs: number };

	/**
	 * One round-trip per renderer. Wall-clock is "first byte queued" →
	 * "renderer reports it has fully drained and painted". For xterm.js
	 * we use its write-callback (parser drain) + 1 RAF (paint commit).
	 * For alacritty we poll frame_seq to detect the exact RAF that did
	 * the render, instead of waiting an arbitrary N frames.
	 */
	async function runOneTrial(payload: Uint8Array, chunkSize: number): Promise<Trial> {
		// Settle the event loop between renderers.
		await nextFrame();

		// --- xterm.js ---
		const tX0 = performance.now();
		for (let off = 0; off < payload.length; off += chunkSize) {
			xtermTerm.write(payload.subarray(off, Math.min(off + chunkSize, payload.length)));
		}
		await xtermFlush();
		await nextFrame();
		const xtermMs = performance.now() - tX0;

		await nextFrame();

		// --- Alacritty WASM ---
		const seqBefore = alacritty.frame_seq();
		const tA0 = performance.now();
		for (let off = 0; off < payload.length; off += chunkSize) {
			alacritty.feed(payload.subarray(off, Math.min(off + chunkSize, payload.length)));
		}
		await waitForAlacrittyRender(seqBefore);
		const alacrittyMs = performance.now() - tA0;
		const parseMs = alacritty.last_parse_ms();
		const renderMs = alacritty.last_render_ms();

		return { xtermMs, alacrittyMs, parseMs, renderMs };
	}

	/**
	 * Run N trials, report median + min/max so the user can see variance.
	 * The previous "single shot" benchmark conflated alacritty's wall-clock
	 * with multiples of the frame budget, which is what made the speedup
	 * number jump around between runs.
	 */
	async function runStress() {
		if (stressRunning || !xtermTerm || !alacritty) return;
		stressRunning = true;
		stressResult = null;
		try {
			// 1MB picks a payload size where alacritty's ~2× speedup is
			// clearly visible. At <500KB the wall-clock is dominated by the
			// half-frame RAF/paint-commit latency (~8ms at 60Hz) on both
			// sides, so small variance flips the winner.
			const payload = buildStressPayload(1024 * 1024);
			const chunkSize = 4 * 1024;
			const trialCount = 5;

			// Warm-up trial — fonts, glyph caches, JIT, etc.
			stressProgress = 'warming up…';
			await runOneTrial(payload.subarray(0, 4096), 1024);

			const trials: Trial[] = [];
			for (let i = 0; i < trialCount; i++) {
				stressProgress = `trial ${i + 1} of ${trialCount}…`;
				trials.push(await runOneTrial(payload, chunkSize));
			}

			const xs = trials.map((t) => t.xtermMs);
			const as = trials.map((t) => t.alacrittyMs);
			const ps = trials.map((t) => t.parseMs);
			const rs = trials.map((t) => t.renderMs);
			const mb = payload.length / (1024 * 1024);
			const xMed = median(xs);
			const aMed = median(as);
			stressResult = {
				bytes: payload.length,
				trials: trialCount,
				xterm: {
					median: xMed,
					min: Math.min(...xs),
					max: Math.max(...xs),
					mbps: mb / (xMed / 1000),
				},
				alacritty: {
					median: aMed,
					min: Math.min(...as),
					max: Math.max(...as),
					mbps: mb / (aMed / 1000),
					parseMedian: median(ps),
					renderMedian: median(rs),
				},
				ratio: xMed / aMed,
			};
			stressProgress = '';
		} finally {
			stressRunning = false;
		}
	}

	function sendInput(bytes: Uint8Array) {
		if (!ws || ws.readyState !== WebSocket.OPEN) return;
		const framed = new Uint8Array(1 + bytes.length);
		framed[0] = MSG_DATA;
		framed.set(bytes, 1);
		ws.send(framed);
		bytesOut += bytes.length;
	}

	function sendResize(cols: number, rows: number) {
		if (!ws || ws.readyState !== WebSocket.OPEN) return;
		const buf = new ArrayBuffer(9);
		const view = new DataView(buf);
		view.setUint8(0, MSG_RESIZE);
		view.setUint16(1, cols, true);
		view.setUint16(3, rows, true);
		view.setUint16(5, 0, true);
		view.setUint16(7, 0, true);
		ws.send(buf);
	}

	onMount(async () => {
		// Kick off config fetch in parallel with wasm init — apply once both ready.
		const userConfigPromise = loadAlacrittyConfig().catch(() => null);

		// --- Alacritty (WASM, Canvas2D) setup ---
		const pkgUrl = new URL('/pkg/alacritty_web.js', window.location.origin).href;
		const wasmModule = await import(/* @vite-ignore */ pkgUrl);
		const w = window as any;
		if (!w.__alacrittyWasmInit) {
			w.__alacrittyWasmInit = wasmModule.default();
		}
		await w.__alacrittyWasmInit;

		alacritty = new wasmModule.AlacrittyTerminal(alacrittyCanvas);
		// Test hook: dev-only handle for driving the page from Playwright.
		if (import.meta.env.DEV) {
			(window as any).__cmp = {
				alacritty,
				get xterm() { return xtermTerm; },
				get ws() { return ws; },
				sendInput,
			};
		}

		// --- xterm.js setup ---
		const { Terminal } = await import('@xterm/xterm');
		const { FitAddon } = await import('@xterm/addon-fit');
		xtermTerm = new Terminal({
			fontFamily: "ui-monospace, 'SF Mono', Menlo, Consolas, 'DejaVu Sans Mono', monospace",
			fontSize: 14,
			theme: {
				background: '#1d1f21',
				foreground: '#c5c8c6',
				cursor: '#c5c8c6',
			},
			cursorBlink: false,
			convertEol: false,
			disableStdin: false,
		});
		xtermFit = new FitAddon();
		xtermTerm.loadAddon(xtermFit);
		xtermTerm.open(xtermEl);
		xtermFit.fit();
		xtermTerm.onData((data: string) => {
			sendInput(new TextEncoder().encode(data));
		});

		// Forward keystrokes from the alacritty canvas (it's the focused one more often
		// since xterm.js captures its own keystrokes via its internal textarea).
		alacrittyCanvas.addEventListener('keydown', (e) => {
			// Ctrl/Cmd+Shift+C: copy selection.
			if ((e.ctrlKey || e.metaKey) && e.shiftKey && (e.key === 'C' || e.key === 'c')) {
				const text = alacritty.selection_text?.();
				if (text) {
					navigator.clipboard?.writeText(text).catch(() => {});
					e.preventDefault();
					return;
				}
			}
			// Paste: Ctrl/Cmd+Shift+V, or Cmd+V on mac. The compare page owns
			// its own WebSocket rather than delegating to alacritty's internal
			// client, so we route the pasted bytes through sendInput directly.
			const isPaste =
				((e.ctrlKey || e.metaKey) && e.shiftKey && (e.key === 'V' || e.key === 'v')) ||
				(e.metaKey && !e.shiftKey && (e.key === 'V' || e.key === 'v'));
			if (isPaste) {
				e.preventDefault();
				navigator.clipboard?.readText().then((text) => {
					if (!text) return;
					const enc = new TextEncoder();
					// When the shell asked for bracketed paste (DECSET 2004),
					// wrap so it can distinguish typed from pasted bytes —
					// vim uses this to disable auto-indent on pastes.
					if (alacritty.bracketed_paste?.()) {
						sendInput(enc.encode('\x1b[200~'));
						sendInput(enc.encode(text));
						sendInput(enc.encode('\x1b[201~'));
					} else {
						sendInput(enc.encode(text));
					}
				}).catch(() => {});
				return;
			}
			const modes = alacritty.keyboard_mode_bits?.() ?? 0;
			const bytes = mapKeyToBytes(e, modes);
			if (bytes) {
				e.preventDefault();
				alacritty.scroll_to_bottom();
				sendInput(bytes);
			}
		});

		// Focus tracking: drives the solid-vs-hollow cursor on the wasm side,
		// and forwards `\e[I` / `\e[O` when the shell enabled DECSET 1004
		// (focus reporting — tmux and neovim consume these to keep their
		// own focus state in sync with the host window).
		alacritty.set_focused(document.activeElement === alacrittyCanvas);

		// Sync-scroll: wheel events on either pane move both viewports so the
		// side-by-side comparison stays honest. Pass scrollBoth as the wheel
		// handler's scroll-out so the shared helper drives both panes.
		const scrollBoth = (lineDelta: number) => {
			if (lineDelta === 0) return;
			alacritty.scroll(lineDelta);
			// xterm.js exposes scrollLines(n) where positive n scrolls DOWN
			// towards newer output, which is the opposite sign.
			try { xtermTerm.scrollLines(-lineDelta); } catch {}
		};
		// xterm.js consumes wheel events internally; hook onScroll to mirror
		// its viewport into alacritty.
		let lastXtermYDisp = xtermTerm.buffer.active.viewportY;
		xtermScrollSub = xtermTerm.onScroll(() => {
			const yDisp = xtermTerm.buffer.active.viewportY;
			const diff = yDisp - lastXtermYDisp;
			lastXtermYDisp = yDisp;
			if (diff === 0) return;
			alacritty.scroll(-diff);
		});

		// All mouse / wheel / focus / contextmenu handling lives in the shared
		// helper (see $lib/canvas-handlers.ts) so we don't drift from the /
		// route's behaviour. onScrollDelta routes wheels through scrollBoth.
		unwireCanvas = wireTerminalCanvas(alacrittyCanvas, alacritty, {
			sendInput,
			onScrollDelta: scrollBoth,
		});

		// Sync grid size on both terminals to whichever is visually smaller,
		// so the server sees one consistent size.
		const syncSize = () => {
			if (!alacritty || !xtermTerm) return;
			try { alacritty.sync_canvas_size(); } catch {}
			const cellW = alacritty.cell_width();
			const cellH = alacritty.cell_height();
			const aCols = Math.max(1, Math.floor(alacrittyCanvas.clientWidth / cellW));
			const aRows = Math.max(1, Math.floor(alacrittyCanvas.clientHeight / cellH));
			try { xtermFit.fit(); } catch {}
			const xCols = xtermTerm.cols;
			const xRows = xtermTerm.rows;
			const cols = Math.min(aCols, xCols);
			const rows = Math.min(aRows, xRows);
			alacritty.resize(cols, rows);
			try { xtermTerm.resize(cols, rows); } catch {}
			sendResize(cols, rows);
		};
		requestAnimationFrame(syncSize);
		resizeObserver = new ResizeObserver(syncSize);
		resizeObserver.observe(alacrittyCanvas);
		resizeObserver.observe(xtermEl);

		// --- WebSocket: tee to both ---
		ws = new WebSocket(WS_URL);
		ws.binaryType = 'arraybuffer';
		ws.onopen = () => {
			status = 'open';
			// Tell the server the initial size.
			syncSize();
		};
		ws.onmessage = (ev) => {
			if (!(ev.data instanceof ArrayBuffer)) return;
			const a = new Uint8Array(ev.data);
			if (a.length === 0) return;
			if (a[0] === MSG_DATA) {
				const payload = a.slice(1);
				bytesIn += payload.length;

				// Feed xterm.js and measure the synchronous cost of its write call.
				const tX = performance.now();
				xtermTerm.write(payload);
				xtermRenderMs = performance.now() - tX;

				// Feed alacritty-wasm and measure the synchronous cost of queueing.
				// Actual GPU/canvas render happens on the next RAF.
				const tA = performance.now();
				alacritty.feed(payload);
				alacrittyRenderMs = performance.now() - tA;
			}
		};
		ws.onclose = () => { status = 'closed'; };
		ws.onerror = () => { status = 'error'; };

		// Stash the user's config so the toggle can apply it on click.
		userConfig = await userConfigPromise;
	});

	onDestroy(() => {
		unwireCanvas?.();
		try { xtermScrollSub?.dispose(); } catch {}
		try { resizeObserver?.disconnect(); } catch {}
		try { ws?.close(); } catch {}
		try { xtermTerm?.dispose(); } catch {}
		try { alacritty?.dispose(); } catch {}
	});
</script>

<svelte:head>
	<title>Alacritty WASM vs xterm.js</title>
</svelte:head>

<div class="page">
	<header class="hero">
		<h1>Alacritty <span class="accent">WASM</span> vs xterm.js</h1>
		<p class="sub">
			Same PTY, tee'd into both terminals. Type in either pane — both render the shell's response.
		</p>
		<div class="stats">
			<span class="stat" class:open={status === 'open'} class:err={status === 'error' || status === 'closed'}>
				{status}
			</span>
			<span class="stat">in: {bytesIn}B</span>
			<span class="stat">out: {bytesOut}B</span>
			<span class="stat">xterm write: {xtermRenderMs.toFixed(2)}ms</span>
			<span class="stat">alacritty feed: {alacrittyRenderMs.toFixed(2)}ms</span>
		</div>
		<div class="stress">
			<div class="stress-row">
				<button class="stress-btn" onclick={runStress} disabled={stressRunning}>
					{stressRunning ? (stressProgress || 'Streaming…') : 'Run 1MB stress test'}
				</button>
				{#if userConfig}
					<button class="stress-btn" onclick={toggleUserConfig}>
						{userConfigActive ? 'Reset palette' : 'Apply my alacritty.toml'}
					</button>
					{#if userConfigActive}
						<span class="config-applied">
							{userConfig.fontFamily ?? 'palette only'}{userConfig.fontSize ? ` @ ${userConfig.fontSize}pt` : ''}
						</span>
					{/if}
				{/if}
			</div>
			{#if stressResult}
				<div class="stress-grid">
					<div class="stress-cell">
						<div class="stress-label">payload</div>
						<div class="stress-value">{(stressResult.bytes / 1024).toFixed(0)} KB</div>
						<div class="stress-sub">median of {stressResult.trials} trials</div>
					</div>
					<div class="stress-cell">
						<div class="stress-label xterm-label">xterm.js</div>
						<div class="stress-value">{stressResult.xterm.median.toFixed(1)} ms</div>
						<div class="stress-sub">
							{stressResult.xterm.mbps.toFixed(1)} MB/s · range {stressResult.xterm.min.toFixed(0)}–{stressResult.xterm.max.toFixed(0)}
						</div>
					</div>
					<div class="stress-cell">
						<div class="stress-label alacritty-label">alacritty wasm</div>
						<div class="stress-value">{stressResult.alacritty.median.toFixed(1)} ms</div>
						<div class="stress-sub">
							{stressResult.alacritty.mbps.toFixed(1)} MB/s · range {stressResult.alacritty.min.toFixed(0)}–{stressResult.alacritty.max.toFixed(0)}
						</div>
					</div>
					<div class="stress-cell stress-ratio">
						<div class="stress-label">speedup</div>
						<div class="stress-value">{stressResult.ratio.toFixed(2)}×</div>
						<div class="stress-sub">
							{stressResult.ratio > 1 ? 'alacritty faster' : 'xterm faster'}
						</div>
					</div>
					<div class="stress-cell stress-breakdown">
						<div class="stress-label">alacritty work breakdown</div>
						<div class="stress-value">
							parse {stressResult.alacritty.parseMedian.toFixed(1)}ms ·
							canvas2d render {stressResult.alacritty.renderMedian.toFixed(1)}ms ·
							total cpu {(stressResult.alacritty.parseMedian + stressResult.alacritty.renderMedian).toFixed(1)}ms
						</div>
						<div class="stress-sub">
							Wall-clock minus cpu time ≈ one-frame paint-commit latency (~8–16ms at 60Hz). Canvas2D render is essentially flat per-frame because it only paints the visible viewport.
						</div>
					</div>
				</div>
			{/if}
		</div>
	</header>

	<div class="split">
		<section class="pane">
			<div class="pane-label">
				<span class="dot xterm"></span>
				xterm.js
				<span class="meta">DOM + Canvas renderer</span>
			</div>
			<div class="pane-body">
				<div class="xterm-host" bind:this={xtermEl}></div>
			</div>
		</section>

		<section class="pane">
			<div class="pane-label">
				<span class="dot alacritty"></span>
				Alacritty (WASM)
				<span class="meta">Rust → wasm32 → Canvas2D</span>
			</div>
			<div class="pane-body">
				<canvas
					bind:this={alacrittyCanvas}
					tabindex="0"
					class="alacritty-canvas"
				></canvas>
			</div>
		</section>
	</div>
</div>

<style>
	.page {
		max-width: 1600px;
		margin: 0 auto;
		padding: 1.5rem 1.5rem 3rem;
		color: #c5c8c6;
	}
	.hero {
		text-align: center;
		margin-bottom: 1.25rem;
	}
	.hero h1 {
		font-size: 1.75rem;
		font-weight: 700;
		color: #e0e0e0;
		margin: 0 0 0.25rem;
	}
	.accent { color: #81a2be; }
	.sub {
		color: #969896;
		font-size: 0.9rem;
		margin: 0 0 0.75rem;
	}
	.stats {
		display: flex;
		justify-content: center;
		gap: 0.5rem;
		flex-wrap: wrap;
		font-size: 0.8rem;
	}
	.stat {
		background: #282a2e;
		border: 1px solid #373b41;
		color: #969896;
		padding: 3px 10px;
		border-radius: 10px;
		font-family: ui-monospace, Menlo, monospace;
	}
	.stat.open { border-color: #b5bd68; color: #b5bd68; }
	.stat.err { border-color: #cc6666; color: #cc6666; }

	.stress {
		margin-top: 0.6rem;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 0.6rem;
	}
	.stress-btn {
		background: #373b41;
		color: #c5c8c6;
		border: 1px solid #4d5057;
		padding: 6px 16px;
		border-radius: 6px;
		cursor: pointer;
		font-size: 0.85rem;
		font-family: ui-monospace, Menlo, monospace;
		transition: background 0.15s;
	}
	.stress-btn:hover:not(:disabled) { background: #4d5057; }
	.stress-btn:disabled { opacity: 0.5; cursor: not-allowed; }
	.stress-row {
		display: flex;
		flex-wrap: wrap;
		justify-content: center;
		gap: 0.5rem;
		align-items: center;
	}
	.config-applied {
		font-family: ui-monospace, Menlo, monospace;
		font-size: 0.75rem;
		color: #81a2be;
	}
	.stress-grid {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: 0.5rem;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 0.78rem;
		width: 100%;
		max-width: 760px;
	}
	.stress-cell {
		background: #1f2226;
		border: 1px solid #2a2d33;
		border-radius: 6px;
		padding: 8px 12px;
		text-align: center;
	}
	.stress-cell.stress-ratio {
		background: #1f2628;
		border-color: #335147;
	}
	.stress-cell.stress-breakdown {
		grid-column: span 4;
		background: #1d1f23;
	}
	.stress-label {
		color: #707882;
		font-size: 0.7rem;
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}
	.stress-label.xterm-label { color: #f0c674; }
	.stress-label.alacritty-label { color: #81a2be; }
	.stress-value {
		color: #e0e0e0;
		font-weight: 600;
		font-size: 0.95rem;
		margin-top: 2px;
	}
	.stress-sub {
		color: #969896;
		font-size: 0.7rem;
		margin-top: 2px;
	}

	.split {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 1rem;
	}
	@media (max-width: 900px) {
		.split { grid-template-columns: 1fr; }
	}

	.pane {
		background: #1a1c20;
		border: 1px solid #2a2d33;
		border-radius: 10px;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}
	.pane-label {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		padding: 8px 14px;
		border-bottom: 1px solid #2a2d33;
		font-size: 0.85rem;
		font-weight: 600;
		background: #222428;
	}
	.dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		flex-shrink: 0;
	}
	.dot.xterm { background: #f0c674; }
	.dot.alacritty { background: #81a2be; }
	.meta {
		color: #707882;
		font-weight: 400;
		margin-left: auto;
		font-size: 0.75rem;
	}
	.pane-body {
		height: 560px;
		padding: 6px;
		background: #1d1f21;
	}
	.xterm-host {
		width: 100%;
		height: 100%;
	}
	.alacritty-canvas {
		width: 100%;
		height: 100%;
		display: block;
		outline: none;
	}
</style>
