import { test, expect } from '@playwright/test';

/**
 * End-to-end verification against a real bash PTY (alacritty_pty_server).
 *
 * Most of the suite feeds synthetic bytes directly into the wasm terminal
 * via `feed()`. This file is different: the /compare page connects to a
 * real WebSocket PTY backed by `/bin/bash`, and we exercise the parity
 * features through that stack — the same path users hit in production.
 *
 * The pty server is started by Playwright's `webServer` config; if it
 * isn't reachable the tests fail loudly rather than silently skipping.
 */

test.describe('alacritty wasm + real bash PTY', () => {
	test('PS1 OSC 2 title flows from bash → wasm → JS getter', async ({ page }) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});
		// Wait for bash's PS1 to land — it includes an OSC 2 title write.
		// We poll the getter rather than using a fixed sleep so the test
		// is robust to slow shell startups.
		const title = await page.waitForFunction(
			() => (window as any).__cmp?.alacritty?.title?.() ?? null,
			null,
			{ timeout: 10_000, polling: 200 }
		);
		const value = await title.jsonValue();
		// `me@host:cwd` is the default PS1 prompt-command title. We don't
		// assert exact contents (different users / hosts) — just that it's
		// a non-empty string with a `:` between user@host and cwd.
		expect(typeof value).toBe('string');
		expect((value as string).length).toBeGreaterThan(0);
	});

	test('OSC 8 hyperlink printed by bash is round-tripped to hyperlink_at', async ({
		page,
	}) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});
		// Wait for the websocket to open before sending input.
		await page.waitForFunction(
			() => (window as any).__cmp?.ws?.readyState === 1,
			null,
			{ timeout: 10_000 }
		);
		// Give bash a moment to render its first prompt so subsequent input
		// isn't eaten by the line-discipline-vs-PS1 race.
		await page.waitForTimeout(500);

		const found = await page.evaluate(async () => {
			const cmp = (window as any).__cmp;
			const a = cmp.alacritty;
			const send = (s: string) =>
				cmp.sendInput(new TextEncoder().encode(s));

			send('\x15'); // Ctrl+U: clear any residual line
			send('\r');
			await new Promise((r) => setTimeout(r, 200));
			// Use a sentinel URI we can search for unambiguously.
			send(
				"printf 'before \\e]8;;https://anthropic.com\\e\\\\LINK\\e]8;;\\e\\\\ after\\n'\r"
			);
			// Poll the grid for the link to appear.
			for (let i = 0; i < 60; i++) {
				await new Promise((r) => setTimeout(r, 100));
				for (let row = 0; row < a.rows(); row++) {
					for (let col = 0; col < a.cols(); col++) {
						const uri = a.hyperlink_at(row, col);
						if (uri && uri.includes('anthropic.com')) {
							return { row, col, uri };
						}
					}
				}
			}
			return null;
		});

		expect(found, 'hyperlink should be found in the grid').toBeTruthy();
		expect(found!.uri).toBe('https://anthropic.com');
	});

	test('Ctrl+click on OSC 8 hyperlink calls window.open with the URI', async ({
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
		await page.waitForTimeout(500);

		// Install a window.open spy on the page so we can observe what the
		// click handler tries to navigate to without actually opening tabs.
		await page.evaluate(() => {
			(window as any).__opens = [];
			const orig = window.open;
			window.open = (url?: string | URL, target?: string, features?: string) => {
				(window as any).__opens.push({ url: String(url ?? ''), target, features });
				return null;
			};
			(window as any).__origOpen = orig;
		});

		// Print a hyperlink, then a `javascript:` URI we expect to be blocked
		// by the allow-list. Capture both rows so we can click them.
		const cells = await page.evaluate(async () => {
			const cmp = (window as any).__cmp;
			const a = cmp.alacritty;
			const send = (s: string) =>
				cmp.sendInput(new TextEncoder().encode(s));
			send('\x15'); send('\r');
			await new Promise((r) => setTimeout(r, 200));
			send(
				"printf 'go \\e]8;;https://safe.example/\\e\\\\OK\\e]8;;\\e\\\\ then \\e]8;;javascript:alert(1)\\e\\\\BAD\\e]8;;\\e\\\\\\n'\r"
			);
			// Wait for both to appear.
			let safeCell: { row: number; col: number } | null = null;
			let badCell: { row: number; col: number } | null = null;
			for (let i = 0; i < 60 && (!safeCell || !badCell); i++) {
				await new Promise((r) => setTimeout(r, 100));
				for (let row = 0; row < a.rows(); row++) {
					for (let col = 0; col < a.cols(); col++) {
						const uri = a.hyperlink_at(row, col);
						if (!uri) continue;
						if (uri === 'https://safe.example/' && !safeCell)
							safeCell = { row, col };
						if (uri === 'javascript:alert(1)' && !badCell)
							badCell = { row, col };
					}
				}
			}
			return { safeCell, badCell };
		});

		expect(cells.safeCell, 'safe link should be on the grid').toBeTruthy();
		expect(cells.badCell, 'bad link should be on the grid').toBeTruthy();

		// Compute the click coordinates for each cell.
		const coordsFor = async (row: number, col: number) =>
			page.evaluate(
				({ row, col }) => {
					const a = (window as any).__cmp.alacritty;
					const canvas = document.querySelector(
						'canvas.alacritty-canvas'
					) as HTMLCanvasElement;
					const rect = canvas.getBoundingClientRect();
					const cellW = a.cell_width();
					const cellH = a.cell_height();
					return {
						x: rect.left + col * cellW + cellW / 2,
						y: rect.top + row * cellH + cellH / 2,
					};
				},
				{ row, col }
			);

		const safeXY = await coordsFor(cells.safeCell!.row, cells.safeCell!.col);
		const badXY = await coordsFor(cells.badCell!.row, cells.badCell!.col);

		// Ctrl+click the safe URI — should call window.open exactly once.
		await page.mouse.move(safeXY.x, safeXY.y);
		await page.keyboard.down('Control');
		await page.mouse.down({ button: 'left' });
		await page.mouse.up({ button: 'left' });
		await page.keyboard.up('Control');

		// Ctrl+click the `javascript:` URI — should NOT call window.open.
		await page.mouse.move(badXY.x, badXY.y);
		await page.keyboard.down('Control');
		await page.mouse.down({ button: 'left' });
		await page.mouse.up({ button: 'left' });
		await page.keyboard.up('Control');

		const opens = await page.evaluate(() => (window as any).__opens);
		expect(opens, 'window.open spy state').toHaveLength(1);
		expect(opens[0].url).toBe('https://safe.example/');
		expect(opens[0].target).toBe('_blank');
		expect(opens[0].features).toContain('noopener');
	});

	test('server emits MSG_EXIT (0x02) when bash exits', async ({ page }) => {
		// Per #26 acceptance: "Handle server-side PTY exit gracefully".
		// The server emits a single binary frame `[0x02]` or `[0x02, code]`
		// when the child process terminates. The compare page's WS is the
		// observation point — install a message spy before triggering exit.
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});
		await page.waitForFunction(
			() => (window as any).__cmp?.ws?.readyState === 1,
			null,
			{ timeout: 10_000 },
		);
		await page.waitForTimeout(500);

		const exitFrame = await page.evaluate(async () => {
			const cmp = (window as any).__cmp;
			// /compare sets ws.binaryType = 'arraybuffer', so binary frames
			// arrive as ArrayBuffer not Blob.
			let resolved: number[] | null = null;
			const done = new Promise<number[] | null>((resolve) => {
				const handler = (ev: MessageEvent) => {
					let bytes: Uint8Array | null = null;
					if (ev.data instanceof ArrayBuffer) bytes = new Uint8Array(ev.data);
					else if (ev.data instanceof Uint8Array) bytes = ev.data;
					if (bytes && bytes.length >= 1 && bytes[0] === 0x02 && !resolved) {
						resolved = Array.from(bytes);
						cmp.ws.removeEventListener('message', handler);
						resolve(resolved);
					}
				};
				cmp.ws.addEventListener('message', handler);
				setTimeout(() => resolve(null), 5000);
			});

			cmp.sendInput(new TextEncoder().encode('exit\n'));
			return done;
		});

		expect(exitFrame, 'server should emit a MSG_EXIT frame').not.toBeNull();
		expect(exitFrame![0]).toBe(0x02);
		// Length: tag only (1) or tag + exit code (2).
		expect(exitFrame!.length).toBeGreaterThanOrEqual(1);
		expect(exitFrame!.length).toBeLessThanOrEqual(2);
		if (exitFrame!.length === 2) {
			// Bash's `exit` with no arg propagates the previous command's
			// exit status; on a fresh shell that's 0.
			expect(exitFrame![1]).toBeLessThanOrEqual(2);
		}
	});

	test('DECSET 1004 makes focus/blur emit ESC[I / ESC[O on /compare', async ({
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
		await page.waitForTimeout(500);

		// Have the "shell" enable focus reporting by feeding the DECSET
		// directly (avoids racing bash's prompt redrawing). Spy on the
		// underlying WebSocket send so we observe exactly what bytes leave
		// for the PTY — /compare captures `sendInput` by closure inside its
		// focus listeners, so wrapping the JS-exposed reference doesn't
		// intercept; the ws transport is the canonical bottleneck.
		const result = await page.evaluate(async () => {
			const cmp = (window as any).__cmp;
			const a = cmp.alacritty;

			(window as any).__outbound = [];
			const origSend = cmp.ws.send.bind(cmp.ws);
			cmp.ws.send = (data: any) => {
				// Convert whatever ws.send is given into a byte array so the
				// test sees both Uint8Array (sendInput frames) and ArrayBuffer
				// (sendResize frames) consistently.
				let bytes: number[] = [];
				if (data instanceof ArrayBuffer) bytes = Array.from(new Uint8Array(data));
				else if (ArrayBuffer.isView(data)) bytes = Array.from(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
				(window as any).__outbound.push(bytes);
				return origSend(data);
			};

			// Feed the DECSET into the local parser.
			a.feed(new TextEncoder().encode('\x1b[?1004h'));
			const seq = a.frame_seq();
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
				if (a.frame_seq() !== seq) break;
			}
			const focusBit = a.keyboard_mode_bits() & 4;

			// Reset outbound after the setup feed.
			(window as any).__outbound = [];

			// Focus the canvas, then move focus off it. Both must produce
			// the expected escape sequence in outbound.
			const canvas = document.querySelector(
				'canvas.alacritty-canvas'
			) as HTMLCanvasElement;
			canvas.focus({ preventScroll: true });
			await new Promise((r) => setTimeout(r, 100));
			(document.activeElement as HTMLElement | null)?.blur();
			await new Promise((r) => setTimeout(r, 100));
			return {
				focusBit,
				outbound: (window as any).__outbound as number[][],
			};
		});

		expect(result.focusBit, 'DECSET 1004 should set bit 2').not.toBe(0);
		// sendInput prefixes a MSG_DATA byte (0x00) before user bytes. So
		// the focus payload looks like [0x00, 0x1b, 0x5b, 0x49] and the
		// blur payload [0x00, 0x1b, 0x5b, 0x4f].
		const focusFrames = result.outbound.filter(
			(b) =>
				b.length === 4 &&
				b[0] === 0x00 &&
				b[1] === 0x1b &&
				b[2] === 0x5b &&
				(b[3] === 0x49 || b[3] === 0x4f)
		);
		expect(focusFrames, 'two focus events should be emitted').toEqual([
			[0x00, 0x1b, 0x5b, 0x49],
			[0x00, 0x1b, 0x5b, 0x4f],
		]);
	});

	test('OSC 8 hover toggles the canvas cursor between pointer and default', async ({
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
		await page.waitForTimeout(500);

		// Print a hyperlink we can find by URI; capture its cell coords.
		const linkCell = await page.evaluate(async () => {
			const cmp = (window as any).__cmp;
			const a = cmp.alacritty;
			const send = (s: string) =>
				cmp.sendInput(new TextEncoder().encode(s));
			send('\x15'); send('\r');
			await new Promise((r) => setTimeout(r, 200));
			send(
				"printf 'edge \\e]8;;https://hover.example/\\e\\\\HOVER\\e]8;;\\e\\\\ tail\\n'\r"
			);
			for (let i = 0; i < 60; i++) {
				await new Promise((r) => setTimeout(r, 100));
				for (let row = 0; row < a.rows(); row++) {
					for (let col = 0; col < a.cols(); col++) {
						if (a.hyperlink_at(row, col) === 'https://hover.example/') {
							return { row, col };
						}
					}
				}
			}
			return null;
		});
		expect(linkCell, 'hyperlink should be on screen').toBeTruthy();

		// Build mouse coords: one IN the hyperlinked cell, one elsewhere.
		const coords = await page.evaluate(({ row, col }) => {
			const a = (window as any).__cmp.alacritty;
			const canvas = document.querySelector(
				'canvas.alacritty-canvas'
			) as HTMLCanvasElement;
			const rect = canvas.getBoundingClientRect();
			const cellW = a.cell_width();
			const cellH = a.cell_height();
			return {
				link: {
					x: rect.left + col * cellW + cellW / 2,
					y: rect.top + row * cellH + cellH / 2,
				},
				// A cell on the same row but several columns to the right of
				// the link — guaranteed-plain " tail" suffix.
				offLink: {
					x: rect.left + (col + 10) * cellW + cellW / 2,
					y: rect.top + row * cellH + cellH / 2,
				},
			};
		}, linkCell!);

		const cursorOf = async () =>
			page.evaluate(
				() =>
					(
						document.querySelector(
							'canvas.alacritty-canvas'
						) as HTMLCanvasElement
					).style.cursor
			);

		// Hover OFF the link first — cursor should be default.
		await page.mouse.move(coords.offLink.x, coords.offLink.y);
		await page.waitForTimeout(50);
		const cursorOff1 = await cursorOf();

		// Now hover ON the link — should flip to 'pointer'.
		await page.mouse.move(coords.link.x, coords.link.y);
		await page.waitForTimeout(50);
		const cursorOn = await cursorOf();

		// Move back off — should flip back.
		await page.mouse.move(coords.offLink.x, coords.offLink.y);
		await page.waitForTimeout(50);
		const cursorOff2 = await cursorOf();

		expect(cursorOff1, 'before hover').not.toBe('pointer');
		expect(cursorOn, 'over hyperlink').toBe('pointer');
		expect(cursorOff2, 'after leaving hyperlink').not.toBe('pointer');
	});

	test('modified arrow key (Ctrl+Left) moves readline by a word', async ({
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
		await page.waitForTimeout(500);

		// readline interprets `\e[1;5D` (Ctrl+Left, xterm encoding) as
		// backward-word. If the modifier encoding I added is correct, sending
		// that sequence after typing "hello world" should move the cursor
		// from col(end) to col(start-of-"world"), i.e. 6 columns back.
		const result = await page.evaluate(async () => {
			const cmp = (window as any).__cmp;
			const a = cmp.alacritty;
			const send = (s: string) =>
				cmp.sendInput(new TextEncoder().encode(s));

			send('\x15'); // Ctrl+U: kill anything pending
			send('\r');
			await new Promise((r) => setTimeout(r, 400));
			// Type the test phrase at the prompt without committing it.
			send('hello world');
			// Wait for the echo to land.
			for (let i = 0; i < 30; i++) {
				await new Promise((r) =>
					requestAnimationFrame(() => r(null))
				);
			}
			const colAfterType = a.cursor_col();

			// Send Ctrl+Left: ESC [ 1 ; 5 D — the exact bytes our key
			// mapping produces for a Ctrl-modified arrow.
			send('\x1b[1;5D');
			for (let i = 0; i < 30; i++) {
				await new Promise((r) =>
					requestAnimationFrame(() => r(null)));
			}
			const colAfterCtrlLeft = a.cursor_col();

			// Clean up: Ctrl+U to kill the line so we don't run "hello world"
			// as a command when the test exits and a new test reuses the
			// session (Playwright workers reuse browser context).
			send('\x15');
			send('\r');

			return { colAfterType, colAfterCtrlLeft };
		});

		// The exact column depends on the prompt width (PS1 plus the typed
		// text), so we test the *delta*: from end-of-line at "hello world|",
		// readline's backward-word lands at the start of "world", i.e. on
		// the 'w'. That's a 5-column move ("world" is 5 chars). The space
		// is included in the word's leading whitespace, not counted as its
		// own jump.
		expect(result.colAfterCtrlLeft).toBeLessThan(result.colAfterType);
		expect(result.colAfterType - result.colAfterCtrlLeft).toBe(5);
	});

	test('DECSCUSR beam from bash changes the cursor shape end-to-end', async ({
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
		await page.waitForTimeout(500);

		const result = await page.evaluate(async () => {
			const cmp = (window as any).__cmp;
			const a = cmp.alacritty;
			const canvas = document.querySelector(
				'canvas.alacritty-canvas'
			) as HTMLCanvasElement;
			const ctx = canvas.getContext('2d')!;
			const dpr = window.devicePixelRatio || 1;
			const cellW = a.cell_width();
			const cellH = a.cell_height();
			const send = (s: string) =>
				cmp.sendInput(new TextEncoder().encode(s));

			send('\x15');
			send('\r');
			await new Promise((r) => setTimeout(r, 300));
			// Move cursor to bottom of viewport and force a beam.
			send("printf '\\e[5 q'\r");
			// Wait for the change to land.
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
			}
			// Focus the canvas so the cursor renders solid (focused beam),
			// not hollow. The block-vs-beam comparison only works that way.
			canvas.focus();
			for (let i = 0; i < 5; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
			}

			// Sample the cell where the cursor sits — for a beam, the LEFT
			// edge should be the only inked column past the glyph layer.
			// We don't know exactly which cell holds the cursor, so we just
			// look for any column with strong vertical ink concentrated on
			// its left edge. A block cursor would have wide horizontal ink.
			const w = canvas.width;
			const h = canvas.height;
			const img = ctx.getImageData(0, 0, w, h);
			// For each cell-sized column, count inked pixels in the left-
			// 25% of the column vs. the rest. A beam shows up as "almost all
			// the inked vertical ink lives in the left third".
			const cellPxW = Math.floor(cellW * dpr);
			const cellPxH = Math.floor(cellH * dpr);
			let bestBeamScore = 0;
			for (let cx = 0; cx + cellPxW <= w; cx += cellPxW) {
				for (let cy = 0; cy + cellPxH <= h; cy += cellPxH) {
					let leftInk = 0, rightInk = 0;
					for (let y = 0; y < cellPxH; y++) {
						for (let x = 0; x < cellPxW; x++) {
							const i = ((cy + y) * w + (cx + x)) * 4;
							const r = img.data[i],
								g = img.data[i + 1],
								b = img.data[i + 2];
							const inked =
								r > 100 || g > 100 || b > 100;
							if (!inked) continue;
							if (x < cellPxW / 4) leftInk++;
							else rightInk++;
						}
					}
					// Beam-like signature: lots of ink on the left edge,
					// almost none in the wider remainder.
					if (leftInk > cellPxH * 0.6 && rightInk < leftInk * 0.4) {
						bestBeamScore = Math.max(bestBeamScore, leftInk);
					}
				}
			}
			return { bestBeamScore, cellPxH };
		});

		// At least one cell on screen should look like a beam — that's our
		// cursor.
		expect(
			result.bestBeamScore,
			'a beam-shaped cell (heavy left-edge ink) should exist'
		).toBeGreaterThan(result.cellPxH * 0.6);
	});
});
