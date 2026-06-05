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

		// Smoke budgets — sized to catch true regressions (e.g. a debug build
		// of pkg/ slipping in, where parse_ms would jump to 500ms+), not to
		// guard the best-case median. Baseline on a quiet box is parse≈15ms,
		// render≈1ms, wall-clock≈40ms; ceilings are ~5–10× that to absorb
		// loaded-CI noise without losing the regression-detection intent.
		expect(result.parseMed, 'rust parse_ms on 1MB').toBeLessThan(150);
		expect(result.renderMed, 'canvas2d render_ms per frame').toBeLessThan(10);
		expect(result.alaMed, 'alacritty wall-clock 1MB').toBeLessThan(300);
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

	test('OSC 8 hyperlink lookup returns URI to JS', async ({ page }) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});
		const result = await page.evaluate(async () => {
			const a = (window as any).__cmp.alacritty;
			const enc = new TextEncoder();
			// `\e]8;;<uri>\e\\TEXT\e]8;;\e\\` — minimal OSC 8 form.
			a.feed(enc.encode('foo \x1b]8;;https://example.com\x1b\\bar\x1b]8;;\x1b\\ baz'));
			const seq = a.frame_seq();
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
				if (a.frame_seq() !== seq) break;
			}
			return {
				outsideLink: a.hyperlink_at(0, 0),
				insideLink: a.hyperlink_at(0, 4),
				lastLinkCell: a.hyperlink_at(0, 6),
				afterLink: a.hyperlink_at(0, 8),
				outOfBounds: a.hyperlink_at(0, 9999),
			};
		});
		expect(result.outsideLink).toBeFalsy();
		expect(result.insideLink).toBe('https://example.com');
		expect(result.lastLinkCell).toBe('https://example.com');
		expect(result.afterLink).toBeFalsy();
		expect(result.outOfBounds).toBeFalsy();
	});

	test('OSC 8 hyperlinked cells render with a solid underline', async ({ page }) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});
		const probe = await page.evaluate(async () => {
			const a = (window as any).__cmp.alacritty;
			const enc = new TextEncoder();
			const canvas = document.querySelector(
				'canvas.alacritty-canvas'
			) as HTMLCanvasElement;
			const ctx = canvas.getContext('2d')!;
			const dpr = window.devicePixelRatio || 1;
			const cellW = a.cell_width();
			const cellH = a.cell_height();

			// `LINK` is hyperlinked, `PLAIN` is not. Both must have ink at the
			// glyph area but only `LINK` should have a fully-inked row at the
			// primary underline position.
			a.feed(enc.encode(
				'\x1b]8;;https://example.com\x1b\\LINK\x1b]8;;\x1b\\ PLAIN\r\n'
			));
			const seq = a.frame_seq();
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
				if (a.frame_seq() !== seq) break;
			}

			const sampleRow = (startCol: number, span: number) => {
				const cellPxW = Math.max(1, Math.floor(cellW * dpr));
				const cx = Math.floor(startCol * cellW * dpr);
				const w = span * cellPxW;
				const primaryY = Math.floor((cellH - 2) * dpr);
				const img = ctx.getImageData(cx, primaryY, w, 1);
				let inked = 0;
				for (let i = 0; i < img.data.length; i += 4) {
					if (
						img.data[i] > 80 ||
						img.data[i + 1] > 80 ||
						img.data[i + 2] > 80
					) {
						inked++;
					}
				}
				return { inked, w };
			};

			return {
				link: sampleRow(0, 4),    // "LINK" cells 0..3
				plain: sampleRow(5, 5),    // "PLAIN" cells 5..9
			};
		});
		// Hyperlinked text should have substantial underline coverage.
		expect(probe.link.inked, 'LINK underline coverage').toBeGreaterThan(
			probe.link.w * 0.8
		);
		// Plain text should have nearly none.
		expect(probe.plain.inked, 'PLAIN no underline').toBeLessThan(
			probe.plain.w * 0.2
		);
	});

	test('BEL paints a visible overlay that decays to nothing', async ({ page }) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});
		const result = await page.evaluate(async () => {
			const a = (window as any).__cmp.alacritty;
			const canvas = document.querySelector(
				'canvas.alacritty-canvas'
			) as HTMLCanvasElement;
			const ctx = canvas.getContext('2d')!;
			// Average brightness of a 16x16 sample taken WELL clear of the
			// cursor cell (which sits at the top-left and is rendered as a
			// bright block, polluting the baseline). The bell overlay tints
			// the whole canvas towards white, so brightness should spike
			// right after BEL and return to baseline once the decay finishes.
			const brightness = () => {
				const img = ctx.getImageData(80, 80, 16, 16);
				let sum = 0;
				for (let i = 0; i < img.data.length; i += 4) {
					sum += img.data[i] + img.data[i + 1] + img.data[i + 2];
				}
				return sum / (img.data.length / 4 * 3);
			};
			const waitFrame = () =>
				new Promise<void>((r) => requestAnimationFrame(() => r()));
			// Establish a baseline (whatever the cleared canvas is showing).
			await waitFrame();
			const baseline = brightness();
			// Ring the bell and wait for the very next frame to paint.
			a.feed(new TextEncoder().encode('\x07'));
			for (let i = 0; i < 5; i++) {
				await waitFrame();
				const b = brightness();
				if (b > baseline + 20) {
					// Peak observed. Now let the decay run for ~10 frames and
					// confirm we get back to ≈baseline.
					for (let j = 0; j < 12; j++) await waitFrame();
					const after = brightness();
					return { baseline, peak: b, after };
				}
			}
			return { baseline, peak: baseline, after: baseline };
		});
		// Peak must clearly exceed baseline (overlay is visible).
		expect(result.peak, 'bell peak brightness > baseline').toBeGreaterThan(
			result.baseline + 20
		);
		// And the decay finishes back near baseline (within tolerance — the
		// overlay should be fully gone after 4 frames so 10+ is plenty).
		expect(result.after, 'brightness returns to ~baseline').toBeLessThan(
			result.baseline + 5
		);
	});

	test('paste on /compare wraps text with \\e[200~ when bracketed paste is on', async ({
		page,
	}) => {
		// We bypass the clipboard read (browser security blocks programmatic
		// clipboard access in tests) by stubbing navigator.clipboard.readText
		// to return a known string, then triggering Ctrl+Shift+V.
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});
		await page.waitForFunction(
			() => (window as any).__cmp?.ws?.readyState === 1,
			null,
			{ timeout: 10_000 }
		);

		// Capture every WS frame for inspection.
		await page.evaluate(() => {
			const cmp = (window as any).__cmp;
			(window as any).__outbound = [];
			const orig = cmp.ws.send.bind(cmp.ws);
			cmp.ws.send = (data: any) => {
				let bytes: number[] = [];
				if (data instanceof ArrayBuffer) bytes = Array.from(new Uint8Array(data));
				else if (ArrayBuffer.isView(data))
					bytes = Array.from(
						new Uint8Array(data.buffer, data.byteOffset, data.byteLength)
					);
				(window as any).__outbound.push(bytes);
				return orig(data);
			};
			// Stub the clipboard so paste reads our payload.
			Object.defineProperty(navigator.clipboard, 'readText', {
				configurable: true,
				value: () => Promise.resolve('pasted!'),
			});
		});

		// Helper: feed a DECSET, focus the canvas, fire Ctrl+Shift+V, await
		// the paste's async clipboard read + send, and return outbound.
		const pasteRoundTrip = async (enableBracketed: boolean) => {
			await page.evaluate(async (enable) => {
				const cmp = (window as any).__cmp;
				const a = cmp.alacritty;
				a.feed(new TextEncoder().encode(enable ? '\x1b[?2004h' : '\x1b[?2004l'));
				const seq = a.frame_seq();
				for (let i = 0; i < 30; i++) {
					await new Promise((r) => requestAnimationFrame(() => r(null)));
					if (a.frame_seq() !== seq) break;
				}
				(window as any).__outbound = [];
			}, enableBracketed);
			const canvas = page.locator('canvas.alacritty-canvas');
			await canvas.focus();
			await page.keyboard.press('Control+Shift+V');
			// The paste handler is async (awaits clipboard.readText), so wait
			// for outbound to populate.
			await page.waitForFunction(
				() => ((window as any).__outbound as number[][]).length > 0,
				null,
				{ timeout: 3_000 }
			);
			return await page.evaluate(
				() => (window as any).__outbound as number[][]
			);
		};

		// Bracketed-paste mode: should see exactly three frames — start
		// marker, payload, end marker.
		const withBracket = await pasteRoundTrip(true);
		const decoded = withBracket
			.filter((b) => b.length > 1 && b[0] === 0x00)
			.map((b) => new TextDecoder().decode(new Uint8Array(b.slice(1))));
		expect(decoded, 'three frames: start, payload, end').toEqual([
			'\x1b[200~',
			'pasted!',
			'\x1b[201~',
		]);

		// Plain mode: just the payload, no wrappers.
		const withoutBracket = await pasteRoundTrip(false);
		const decoded2 = withoutBracket
			.filter((b) => b.length > 1 && b[0] === 0x00)
			.map((b) => new TextDecoder().decode(new Uint8Array(b.slice(1))));
		expect(decoded2).toEqual(['pasted!']);
	});

	test('input latency floor — single-byte feed renders inside a frame budget', async ({
		page,
	}) => {
		// Per #28 acceptance: "Input latency under 16ms". Real keystroke
		// latency includes PTY roundtrip + shell echo, which is noisy in CI.
		// This test measures the wasm-side floor only: feed(byte) → next
		// frame_seq increment. Catches catastrophic regressions in the
		// JS↔WASM boundary, parse path, or render loop.
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});

		const samples = await page.evaluate(async () => {
			const a = (window as any).__cmp.alacritty;
			// Warm the wasm + raf pipeline so the first sample isn't tainted
			// by JIT / cold-cache effects.
			a.feed(new TextEncoder().encode('warmup\r\n'));
			for (let i = 0; i < 20; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
			}

			const result: number[] = [];
			for (let trial = 0; trial < 10; trial++) {
				const seqBefore = a.frame_seq();
				const t0 = performance.now();
				a.feed(new TextEncoder().encode('X'));
				while (a.frame_seq() === seqBefore) {
					await new Promise((r) => requestAnimationFrame(() => r(null)));
				}
				result.push(performance.now() - t0);
			}
			result.sort((x, y) => x - y);
			return {
				min: result[0],
				median: result[Math.floor(result.length / 2)],
				p90: result[Math.floor(result.length * 0.9)],
				max: result[result.length - 1],
			};
		});

		console.log(
			`input latency: min=${samples.min.toFixed(2)}ms median=${samples.median.toFixed(
				2,
			)}ms p90=${samples.p90.toFixed(2)}ms max=${samples.max.toFixed(2)}ms`,
		);

		// Most samples should land within one or two RAF ticks (~16-32ms at
		// 60Hz). Headless chromium throttles RAF — cap the median at 100ms
		// as a regression guard, p90 at 200ms.
		expect(samples.median, 'median single-byte feed latency').toBeLessThan(100);
		expect(samples.p90, 'p90 single-byte feed latency').toBeLessThan(200);
	});

	test('mouse-reporting on /compare forwards click + wheel to PTY (not selection/scroll)', async ({
		page,
	}) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});
		await page.waitForFunction(
			() => (window as any).__cmp?.ws?.readyState === 1,
			null,
			{ timeout: 10_000 }
		);

		// Enable mouse click reporting (DECSET 1000) + SGR encoding (1006)
		// via local feed so we don't need a real shell. Capture the canvas
		// geometry for synthesised events.
		const geom = await page.evaluate(async () => {
			const cmp = (window as any).__cmp;
			const a = cmp.alacritty;
			// Spy on the binary protocol so we observe outgoing bytes.
			(window as any).__outbound = [];
			const origSend = cmp.ws.send.bind(cmp.ws);
			cmp.ws.send = (data: any) => {
				let bytes: number[] = [];
				if (data instanceof ArrayBuffer) bytes = Array.from(new Uint8Array(data));
				else if (ArrayBuffer.isView(data))
					bytes = Array.from(
						new Uint8Array(data.buffer, data.byteOffset, data.byteLength)
					);
				(window as any).__outbound.push(bytes);
				return origSend(data);
			};

			a.feed(new TextEncoder().encode('\x1b[?1000h\x1b[?1006h'));
			const seq = a.frame_seq();
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
				if (a.frame_seq() !== seq) break;
			}
			// Reset the spy after the local DECSET — only count subsequent traffic.
			(window as any).__outbound = [];

			const canvas = document.querySelector(
				'canvas.alacritty-canvas'
			) as HTMLCanvasElement;
			const rect = canvas.getBoundingClientRect();
			return {
				rect: { x: rect.x, y: rect.y },
				cellW: a.cell_width(),
				cellH: a.cell_height(),
				active: a.mouse_reporting_active(),
			};
		});
		expect(geom.active, 'reporting should be active after DECSET').toBeTruthy();

		// Click at col 4 row 2 (1-based wire coords: col=5, row=3). SGR
		// encoding makes this `\e[<0;5;3M` on press and `\e[<0;5;3m` on release,
		// each MSG_DATA-framed.
		const x = geom.rect.x + 4 * geom.cellW + geom.cellW / 2;
		const y = geom.rect.y + 2 * geom.cellH + geom.cellH / 2;
		await page.mouse.move(x, y);
		await page.mouse.down({ button: 'left' });
		await page.mouse.up({ button: 'left' });

		// Wheel up at the same spot — should emit button 64 (wheel-up) instead
		// of doing local scrollback.
		await page.mouse.wheel(0, -50);

		const outbound = await page.evaluate(() => (window as any).__outbound as number[][]);
		const decoded = outbound
			.filter((b) => b.length > 1 && b[0] === 0x00)
			.map((b) => new TextDecoder().decode(new Uint8Array(b.slice(1))));
		// At minimum: a press, a release, and a wheel.
		expect(decoded, 'should see SGR mouse sequences').toContain('\x1b[<0;5;3M');
		expect(decoded, 'should see SGR release').toContain('\x1b[<0;5;3m');
		// Wheel-up encodes as button 64. Cell row depends on where wheel
		// lands (mouse hasn't moved since), so we use a regex match.
		const wheelRe = /^\x1b\[<64;\d+;\d+M$/;
		expect(
			decoded.some((s) => wheelRe.test(s)),
			`wheel-up button 64 expected, got ${JSON.stringify(decoded)}`
		).toBe(true);
	});

	test('Alt+drag on /compare creates a rectangular selection', async ({ page }) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});

		// Feed a 5x3 grid of distinct chars synthetically and capture the
		// canvas rect + cell metrics so we know exactly where to click.
		// CSI 2J + cursor-home first because /compare is wired to a live bash
		// PTY that's already printed a prompt — without clearing, the visible
		// rows 0..2 belong to the prompt, not to our test text.
		const geom = await page.evaluate(async () => {
			const a = (window as any).__cmp.alacritty;
			const enc = new TextEncoder();
			a.feed(enc.encode('\x1b[2J\x1b[HABCDE\r\nFGHIJ\r\nKLMNO\r\n'));
			// Wait for a render frame so cursor + grid are stable.
			const seq = a.frame_seq();
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
				if (a.frame_seq() !== seq) break;
			}
			const canvas = document.querySelector(
				'canvas.alacritty-canvas'
			) as HTMLCanvasElement;
			const rect = canvas.getBoundingClientRect();
			return {
				rect: { x: rect.left, y: rect.top },
				cellW: a.cell_width(),
				cellH: a.cell_height(),
			};
		});

		// Block-select cols 1..3 of rows 0..2 → "BCD\nGHI\nLMN".
		// Side-of-cell matters for selection bounds: the begin click needs
		// the left half (so col 1 is included), the end click needs the
		// right half (so col 3 is also included).
		const cellLeft = (row: number, col: number) => ({
			x: geom.rect.x + col * geom.cellW + geom.cellW * 0.25,
			y: geom.rect.y + row * geom.cellH + geom.cellH / 2,
		});
		const cellRight = (row: number, col: number) => ({
			x: geom.rect.x + col * geom.cellW + geom.cellW * 0.75,
			y: geom.rect.y + row * geom.cellH + geom.cellH / 2,
		});
		const start = cellLeft(0, 1);
		const end = cellRight(2, 3);

		await page.keyboard.down('Alt');
		await page.mouse.move(start.x, start.y);
		await page.mouse.down({ button: 'left' });
		// Drag in steps so the move handler fires intermediate updates.
		await page.mouse.move((start.x + end.x) / 2, (start.y + end.y) / 2);
		await page.mouse.move(end.x, end.y);
		await page.mouse.up({ button: 'left' });
		await page.keyboard.up('Alt');

		const text = await page.evaluate(() => {
			const a = (window as any).__cmp.alacritty;
			return a.selection_text();
		});
		expect(text).toBe('BCD\nGHI\nLMN');
	});

	test('forwards OSC 0/2 title to the JS getter', async ({ page }) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});
		const result = await page.evaluate(async () => {
			const a = (window as any).__cmp.alacritty;
			const enc = new TextEncoder();
			const before = a.title();
			// OSC 2 ; <text> BEL — most common form a shell uses.
			a.feed(enc.encode('\x1b]2;hello-from-osc\x07'));
			// Wait for the next render frame to flush the parser.
			const seq = a.frame_seq();
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
				if (a.frame_seq() !== seq) break;
			}
			const after = a.title();
			// OSC 0 sets both icon-name and title; ensure that overwrites too.
			a.feed(enc.encode('\x1b]0;second-title\x07'));
			const seq2 = a.frame_seq();
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
				if (a.frame_seq() !== seq2) break;
			}
			const overwrite = a.title();
			return { before, after, overwrite };
		});
		expect(result.before).toBeFalsy();
		expect(result.after).toBe('hello-from-osc');
		expect(result.overwrite).toBe('second-title');
	});

	test('renderer distinguishes underline styles (solid/double/dotted/dashed/curly)', async ({ page }) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});

		const probe = await page.evaluate(async () => {
			const a = (window as any).__cmp.alacritty;
			const enc = new TextEncoder();
			const canvas = document.querySelector(
				'canvas.alacritty-canvas'
			) as HTMLCanvasElement;
			const ctx = canvas.getContext('2d')!;
			const dpr = window.devicePixelRatio || 1;
			const cellW = a.cell_width();
			const cellH = a.cell_height();

			// SGR 4:n controls underline style — 1=single, 2=double, 3=curly,
			// 4=dotted, 5=dashed. Use "SOLI" / "DOUB" / etc. and a trailing
			// `\e[24m` to terminate the underline so the space between runs is
			// not underlined.
			const CSI = '\x1b[';
			const line =
				`${CSI}4:1mSOLI${CSI}24m ` +
				`${CSI}4:2mDOUB${CSI}24m ` +
				`${CSI}4:3mCURL${CSI}24m ` +
				`${CSI}4:4mDOTT${CSI}24m ` +
				`${CSI}4:5mDASH${CSI}24m\r\n`;
			const seq = a.frame_seq();
			a.feed(enc.encode(line));
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
				if (a.frame_seq() !== seq) break;
			}

			// Per style we measure three things, all inside a 4-cell strip:
			//   primary  = horizontal coverage at the row a *solid* underline sits on
			//   upper    = horizontal coverage two pixels higher (where double's
			//              second bar lives, and curly's wave passes through)
			//   distinct = number of distinct y-rows that contain *any* ink
			//              inside a 6-px vertical band; curly should hit ≥ 3
			//              rows while solid stays at 1.
			// These three are enough to separate every style without depending
			// on glyph descenders, which jitter across fonts and zoom levels.
			const sample = (startCol: number) => {
				const cellPxW = Math.max(1, Math.floor(cellW * dpr));
				const cx = Math.floor(startCol * cellW * dpr);
				const w = 4 * cellPxW;
				const primaryY = Math.floor((cellH - 2) * dpr);
				const upperY = Math.max(0, primaryY - 2);
				const bandTop = Math.max(0, primaryY - 5);
				const bandHeight = Math.max(1, primaryY + 2 - bandTop);
				const img = ctx.getImageData(cx, bandTop, w, bandHeight);
				const inked = (yLocal: number, x: number) => {
					const i = (yLocal * w + x) * 4;
					return (
						img.data[i] > 80 ||
						img.data[i + 1] > 80 ||
						img.data[i + 2] > 80
					);
				};
				let primary = 0, upper = 0;
				const distinctRows = new Set<number>();
				for (let y = 0; y < bandHeight; y++) {
					let rowInk = 0;
					for (let x = 0; x < w; x++) {
						if (inked(y, x)) {
							rowInk++;
							if (y + bandTop === primaryY) primary++;
							if (y + bandTop === upperY) upper++;
						}
					}
					if (rowInk > 0) distinctRows.add(y);
				}
				return {
					primary,
					upper,
					distinctRows: distinctRows.size,
					w,
				};
			};

			return {
				solid: sample(0),
				double: sample(5),
				curly: sample(10),
				dotted: sample(15),
				dashed: sample(20),
			};
		});

		// Solid: the primary row is essentially fully inked across 4 cells.
		expect(probe.solid.primary, 'solid primary-row coverage').toBeGreaterThan(
			probe.solid.w * 0.8
		);
		// Double: primary AND upper rows both have substantial coverage.
		// (Solid has near-zero ink at the upper row because that area sits
		// between glyph descenders and the underline bar.)
		expect(probe.double.primary, 'double primary-row coverage').toBeGreaterThan(
			probe.double.w * 0.6
		);
		expect(
			probe.double.upper,
			'double upper-row coverage (second bar)'
		).toBeGreaterThan(probe.double.w * 0.4);
		expect(
			probe.double.upper,
			'double upper > solid upper'
		).toBeGreaterThan(probe.solid.upper * 2);
		// Curly: the wave should touch noticeably more distinct y-rows than
		// any straight-line style.
		expect(probe.curly.distinctRows, 'curly distinct y-rows').toBeGreaterThan(
			probe.solid.distinctRows
		);
		// Dotted and dashed have gaps along the primary row.
		expect(
			probe.dotted.primary,
			'dotted primary-row coverage < solid'
		).toBeLessThan(probe.solid.primary);
		expect(
			probe.dashed.primary,
			'dashed primary-row coverage < solid'
		).toBeLessThan(probe.solid.primary);
	});

	test('renderer honors DECTCEM and DECSCUSR cursor shapes', async ({ page }) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});

		const probe = await page.evaluate(async () => {
			const a = (window as any).__cmp.alacritty;
			const enc = new TextEncoder();
			const canvas = document.querySelector(
				'canvas.alacritty-canvas'
			) as HTMLCanvasElement;
			const ctx = canvas.getContext('2d')!;
			const dpr = window.devicePixelRatio || 1;
			const cellW = a.cell_width();
			const cellH = a.cell_height();

			const wait = async () => {
				const seq = a.frame_seq();
				for (let i = 0; i < 30; i++) {
					await new Promise((r) => requestAnimationFrame(() => r(null)));
					if (a.frame_seq() !== seq) return;
				}
			};

			// Count "inked" pixels (substantially brighter than dark background)
			// in cell (col, row) of the backing store.
			const cellInk = (col: number, row: number) => {
				const cx = Math.floor(col * cellW * dpr);
				const cy = Math.floor(row * cellH * dpr);
				const w = Math.max(1, Math.floor(cellW * dpr));
				const h = Math.max(1, Math.floor(cellH * dpr));
				const img = ctx.getImageData(cx, cy, w, h);
				let inked = 0, leftHalf = 0;
				for (let i = 0; i < img.data.length; i += 4) {
					const r = img.data[i], g = img.data[i + 1], b = img.data[i + 2];
					if (r > 80 || g > 80 || b > 80) {
						inked++;
						const px = (i / 4) % w;
						if (px < w / 3) leftHalf++;
					}
				}
				return { inked, leftHalf, w, h };
			};

			// Make sure cursor is shown and at origin.
			a.feed(enc.encode('\x1b[?25h\x1b[2 q\x1b[H'));
			await wait();
			const block = cellInk(0, 0);

			// Beam: thin vertical bar at left edge.
			a.feed(enc.encode('\x1b[5 q'));
			await wait();
			const beam = cellInk(0, 0);

			// Underline: thin horizontal bar at bottom.
			a.feed(enc.encode('\x1b[3 q'));
			await wait();
			const underline = cellInk(0, 0);

			// Hidden: nothing in the cell.
			a.feed(enc.encode('\x1b[?25l'));
			await wait();
			const hidden = cellInk(0, 0);

			return { block, beam, underline, hidden };
		});

		// Block fills most of the cell.
		expect(probe.block.inked, 'block cursor inked area').toBeGreaterThan(
			probe.block.w * probe.block.h * 0.5
		);
		// Beam is a thin vertical bar — much less ink than block, and
		// concentrated on the left third of the cell.
		expect(probe.beam.inked, 'beam cursor inked area').toBeLessThan(
			probe.block.inked / 3
		);
		expect(probe.beam.inked, 'beam cursor visible').toBeGreaterThan(0);
		expect(probe.beam.leftHalf / Math.max(1, probe.beam.inked)).toBeGreaterThan(0.9);
		// Underline is also much less ink than block.
		expect(probe.underline.inked, 'underline cursor inked area').toBeLessThan(
			probe.block.inked / 3
		);
		expect(probe.underline.inked, 'underline cursor visible').toBeGreaterThan(0);
		// Hidden draws nothing.
		expect(probe.hidden.inked, 'hidden cursor leaves no ink').toBe(0);
	});
});
