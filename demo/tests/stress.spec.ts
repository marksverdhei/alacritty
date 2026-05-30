import { test, expect } from '@playwright/test';

/**
 * Stress benchmark regression guard.
 *
 * Loads the /compare page (which exposes a `window.__cmp` test hook in dev),
 * runs the same stress payload the in-page benchmark uses, and asserts on
 * the instrumented Rust-side timings (parse_ms, render_ms) that don't depend
 * on the browser's RAF scheduler. Headless chromium throttles
 * requestAnimationFrame, which makes wall-clock comparisons against xterm.js
 * (whose write callback is synchronous) unreliable in CI — so we guard CPU
 * work directly:
 *   - canvas2d render stays under 3ms per frame (huge regression here means
 *     a real algorithmic bug, not a measurement glitch)
 *   - rust-side parse time on 1MB stays under 30ms (catches the VTE getting
 *     a 2-3× slower, e.g. a debug build slipping in)
 *
 * Wall-clock is logged for human eyeballing but not asserted.
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
		seed ^= seed << 13;
		seed ^= seed >>> 17;
		seed ^= seed << 5;
		return (seed >>> 0) / 0xffffffff;
	};
	let out = '';
	while (out.length < targetBytes) {
		const sgr: string[] = [];
		const r = rand();
		if (r < 0.4) sgr.push(`3${Math.floor(rand() * 8)}`);
		else if (r < 0.6) sgr.push(`38;5;${16 + Math.floor(rand() * 200)}`);
		else if (r < 0.7) sgr.push(`38;2;${Math.floor(rand() * 256)};${Math.floor(rand() * 256)};${Math.floor(rand() * 256)}`);
		const r2 = rand();
		if (r2 < 0.3) sgr.push(`4${Math.floor(rand() * 8)}`);
		else if (r2 < 0.45) sgr.push(`48;5;${16 + Math.floor(rand() * 200)}`);
		if (rand() < 0.25) sgr.push('1');
		if (rand() < 0.15) sgr.push('4');
		const prefix = sgr.length ? `${CSI}${sgr.join(';')}m` : '';
		out += `${prefix}${words[Math.floor(rand() * words.length)]}${CSI}0m `;
		if (rand() < 0.06) out += '\r\n';
	}
	return new TextEncoder().encode(out);
}

test.describe('alacritty wasm stress benchmark', () => {
	test('1MB payload — parse + render budgets', async ({ page }) => {
		await page.goto('/compare');

		// Wait for the wasm handle to be wired by the dev-only __cmp hook.
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});

		const payload = buildStressPayload(1024 * 1024);
		// Smuggle the payload over the wire as a regular number[] — the browser
		// reconstructs the Uint8Array inside `page.evaluate`.
		const payloadArr = Array.from(payload);

		const result = await page.evaluate(async (bytesArr: number[]) => {
			const a = (window as any).__cmp.alacritty;
			const xterm = (window as any).__cmp.xterm;
			const payload = new Uint8Array(bytesArr);

			const xtermFlush = () =>
				new Promise<void>((r) => xterm.write('', () => r()));
			const nextFrame = () =>
				new Promise<void>((r) => requestAnimationFrame(() => r()));
			const waitForAla = async (seqBefore: number, timeoutMs = 10_000) => {
				const startT = performance.now();
				while (
					a.frame_seq() === seqBefore ||
					(a.pending_bytes ? a.pending_bytes() : a.pending_chunks()) > 0
				) {
					await nextFrame();
					if (performance.now() - startT > timeoutMs) return false;
				}
				return true;
			};

			async function trial(p: Uint8Array, chunk = 4096) {
				await nextFrame();
				const tx0 = performance.now();
				for (let o = 0; o < p.length; o += chunk) {
					xterm.write(p.subarray(o, Math.min(o + chunk, p.length)));
				}
				await xtermFlush();
				await nextFrame();
				const xtermMs = performance.now() - tx0;
				await nextFrame();

				const seqB = a.frame_seq();
				const ta0 = performance.now();
				for (let o = 0; o < p.length; o += chunk) {
					a.feed(p.subarray(o, Math.min(o + chunk, p.length)));
				}
				await waitForAla(seqB);
				const alaMs = performance.now() - ta0;
				return {
					xtermMs,
					alaMs,
					parse: a.last_parse_ms(),
					render: a.last_render_ms(),
				};
			}
			const median = (vs: number[]) => {
				const s = [...vs].sort((x, y) => x - y);
				const m = Math.floor(s.length / 2);
				return s.length % 2 === 0 ? (s[m - 1] + s[m]) / 2 : s[m];
			};

			// Warm-up
			await trial(payload.subarray(0, 4096), 1024);

			const trials: Array<{
				xtermMs: number;
				alaMs: number;
				parse: number;
				render: number;
			}> = [];
			for (let i = 0; i < 5; i++) trials.push(await trial(payload));

			return {
				xtermMed: median(trials.map((t) => t.xtermMs)),
				alaMed: median(trials.map((t) => t.alaMs)),
				parseMed: median(trials.map((t) => t.parse)),
				renderMed: median(trials.map((t) => t.render)),
				ratio:
					median(trials.map((t) => t.xtermMs)) /
					median(trials.map((t) => t.alaMs)),
			};
		}, payloadArr);

		console.log(
			`benchmark: xterm=${result.xtermMed.toFixed(1)}ms alacritty=${result.alaMed.toFixed(1)}ms ` +
				`(parse=${result.parseMed.toFixed(1)}ms render=${result.renderMed.toFixed(1)}ms) ` +
				`speedup=${result.ratio.toFixed(2)}×`
		);

		// CPU-time assertions: independent of RAF throttling.
		expect(result.parseMed, 'rust parse_ms on 1MB').toBeLessThan(30);
		expect(result.renderMed, 'canvas2d render_ms per frame').toBeLessThan(3);
		// Sanity-only on the noisy wall-clock metric: we just want to catch a
		// massive regression (e.g. 5x slower).
		expect(result.alaMed, 'alacritty wall-clock 1MB').toBeLessThan(80);
	});

	test('mouse passthrough — SGR encoding round-trip', async ({ page }) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});
		const result = await page.evaluate(async () => {
			const a = (window as any).__cmp.alacritty;
			// Off by default.
			const offBytes = a.report_mouse(0, 0, 4, 2, 0);
			const reportingOff = a.mouse_reporting_active();
			// Enable DECSET 1000 (click) + 1006 (SGR).
			a.feed(new TextEncoder().encode('\x1b[?1000h\x1b[?1006h'));
			// Wait one frame so the parser advances.
			for (let i = 0; i < 5; i++) await new Promise((r) => requestAnimationFrame(() => r(null)));
			const reportingOn = a.mouse_reporting_active();
			// Left-click at (col=4, row=2), no mods. Expect "\e[<0;5;3M" in SGR mode.
			const bytes = a.report_mouse(0, 0, 4, 2, 0);
			const asStr = bytes ? new TextDecoder().decode(new Uint8Array(bytes)) : null;
			// Motion event without a held button + only click mode → should be filtered out.
			const motionBytes = a.report_mouse(3, 2, 4, 2, 0);
			return { offBytes, reportingOff, reportingOn, asStr, motionBytes };
		});
		// wasm-bindgen renders `Option::None` as `undefined` on the JS side.
		expect(result.offBytes).toBeFalsy();
		expect(result.reportingOff).toBe(false);
		expect(result.reportingOn).toBe(true);
		expect(result.asStr).toBe('\x1b[<0;5;3M');
		expect(result.motionBytes).toBeFalsy();
	});

	test('feed → render correctness (cursor + visible char)', async ({ page }) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});

		const result = await page.evaluate(async () => {
			const a = (window as any).__cmp.alacritty;
			const seqBefore = a.frame_seq();
			a.feed(new TextEncoder().encode('hello\r\n'));
			// Wait for the parser+renderer to land it.
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
				if (a.frame_seq() !== seqBefore) break;
			}
			// Read pixels at the upper-left of the canvas; the 'h' glyph at (0,0)
			// must have something other than the background colour.
			const canvas = document.querySelector(
				'canvas.alacritty-canvas'
			) as HTMLCanvasElement;
			const ctx = canvas.getContext('2d')!;
			const cellW = a.cell_width();
			const cellH = a.cell_height();
			// Sample a few pixels in the cell to find any non-background pixel.
			const cellPixels = ctx.getImageData(
				1,
				1,
				Math.max(1, Math.floor(cellW)),
				Math.max(1, Math.floor(cellH))
			);
			let nonBg = 0;
			// Default Alacritty bg ≈ rgb(29,31,33). Treat any pixel substantially
			// brighter than that as a "glyph pixel".
			for (let i = 0; i < cellPixels.data.length; i += 4) {
				const r = cellPixels.data[i];
				const g = cellPixels.data[i + 1];
				const b = cellPixels.data[i + 2];
				if (r + g + b > 250) nonBg++;
			}
			return { advancedFrame: a.frame_seq() !== seqBefore, nonBg };
		});

		expect(result.advancedFrame, 'render loop processed the feed').toBe(true);
		expect(result.nonBg, 'glyph pixels rendered in the first cell').toBeGreaterThan(5);
	});

	test('renderer honors SGR styles (bold + ANSI colours)', async ({ page }) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});

		const probe = await page.evaluate(async () => {
			const a = (window as any).__cmp.alacritty;
			const enc = new TextEncoder();
			// Row 0: ESC[31m → red, ESC[32m → green, ESC[34m → blue. Use
			// distinctive double-letter blocks so anti-aliasing has a fat
			// enough run to make the centre pixels honest.
			const CSI = '\x1b[';
			const line = `${CSI}31mRR${CSI}32mGG${CSI}34mBB${CSI}0m\r\n${CSI}1mBOLD${CSI}0m ${CSI}3mITAL${CSI}0m\r\n`;
			const seqBefore = a.frame_seq();
			a.feed(enc.encode(line));
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
				if (a.frame_seq() !== seqBefore) break;
			}

			const canvas = document.querySelector(
				'canvas.alacritty-canvas'
			) as HTMLCanvasElement;
			const ctx = canvas.getContext('2d')!;
			const dpr = window.devicePixelRatio || 1;
			const cellW = a.cell_width();
			const cellH = a.cell_height();

			// Sum the inked (non-background) pixels' channels inside a given
			// grid cell. "Inked" = any pixel substantially brighter than the
			// background. We work in backing-store pixels (ctx.getImageData
			// uses backing pixels, not CSS pixels), so multiply by dpr.
			const sample = (col: number, row: number) => {
				const cx = Math.floor(col * cellW * dpr);
				const cy = Math.floor(row * cellH * dpr);
				const w = Math.max(1, Math.floor(cellW * dpr));
				const h = Math.max(1, Math.floor(cellH * dpr));
				const img = ctx.getImageData(cx, cy, w, h);
				let r = 0, g = 0, b = 0, inked = 0;
				for (let i = 0; i < img.data.length; i += 4) {
					const pr = img.data[i];
					const pg = img.data[i + 1];
					const pb = img.data[i + 2];
					// Skip background-ish pixels.
					if (pr + pg + pb < 120) continue;
					r += pr; g += pg; b += pb; inked++;
				}
				return { r, g, b, inked };
			};

			const red   = sample(0, 0); // 'R'
			const green = sample(2, 0); // 'G'
			const blue  = sample(4, 0); // 'B'
			const bold  = sample(0, 1); // 'B' in BOLD
			const ital  = sample(5, 1); // 'I' in ITAL
			return { red, green, blue, bold, ital };
		});

		// Colour assertions: the dominant inked channel for each glyph must
		// be the one its SGR sequence selected. Tolerate anti-aliasing by
		// asserting "dominant by a clear margin" rather than exact equality.
		expect(probe.red.inked, 'red cell ink').toBeGreaterThan(5);
		expect(probe.red.r).toBeGreaterThan(probe.red.g + probe.red.inked); // ~1+ R per pixel above G
		expect(probe.red.r).toBeGreaterThan(probe.red.b + probe.red.inked);

		expect(probe.green.inked, 'green cell ink').toBeGreaterThan(5);
		expect(probe.green.g).toBeGreaterThan(probe.green.r + probe.green.inked);
		expect(probe.green.g).toBeGreaterThan(probe.green.b + probe.green.inked);

		expect(probe.blue.inked, 'blue cell ink').toBeGreaterThan(5);
		expect(probe.blue.b).toBeGreaterThan(probe.blue.r + probe.blue.inked);
		expect(probe.blue.b).toBeGreaterThan(probe.blue.g + probe.blue.inked);

		// Bold and italic glyphs should both have substantial ink — they're
		// foreground-default text, which means the assertion is "we painted
		// something there". A regression that broke the font-style switch
		// would zero out the run or place text at the wrong y-offset.
		expect(probe.bold.inked, 'bold "B" ink').toBeGreaterThan(8);
		expect(probe.ital.inked, 'italic "I" ink').toBeGreaterThan(4);
	});
});
