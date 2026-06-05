import { test, expect } from '@playwright/test';

/**
 * Visual regression tests using Playwright's `toHaveScreenshot()`.
 *
 * Complements the pixel-sampling tests in stress.spec.ts: those assert
 * semantic things (cell N has substantial inked area, underline row is
 * 80% covered, etc.) and survive pixel-level drift. These tests assert
 * the canvas matches a baseline image, so they catch ANY change a
 * pixel-sampling test might miss — wrong cursor color, slight bar
 * thickness change, accidental anti-aliasing tweak.
 *
 * Trade-off: baselines are environment-sensitive. We use a generous
 * `maxDiffPixels` so subpixel font rendering drift across chromium
 * versions doesn't trip the test. If CI's chromium produces baselines
 * that drift from local ones, regenerate via `npx playwright test
 * --update-snapshots visual.spec.ts` in CI.
 */

test.describe('visual regression', () => {
	test.use({
		// `toHaveScreenshot` tolerance: ~0.2% of pixels can differ.
		// Generous enough to absorb font-AA jitter, tight enough to catch
		// a wrong color or missing decoration.
	});

	// Helper: load /compare, kill the bash session so PS1 doesn't pollute,
	// then feed bytes locally and wait for render. Killing the WS is the
	// only way to make the baseline deterministic — different users have
	// different PS1 (hostname, cwd, starship prompt segments).
	const setup = async (page, content: string) => {
		await page.goto('/compare');
		await page.waitForFunction(() => Boolean((window as any).__cmp?.alacritty), {
			timeout: 15_000,
		});
		await page.waitForFunction(
			() => (window as any).__cmp?.ws?.readyState === 1,
			null,
			{ timeout: 10_000 },
		);
		// Close the PTY WebSocket, wait for it to settle, then drain any
		// in-flight bash output by waiting a couple of frames. After that
		// CSI 2J + cursor-home clears the buffer and the feed is what
		// stays on screen.
		await page.evaluate(async (text) => {
			const cmp = (window as any).__cmp;
			const a = cmp.alacritty;
			cmp.ws.close();
			await new Promise((r) => setTimeout(r, 200));
			// Drain a few frames so any bash output already in flight lands
			// before we clear.
			for (let i = 0; i < 5; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
			}
			a.feed(new TextEncoder().encode('\x1b[2J\x1b[H' + text));
			const seq = a.frame_seq();
			for (let i = 0; i < 30; i++) {
				await new Promise((r) => requestAnimationFrame(() => r(null)));
				if (a.frame_seq() !== seq) break;
			}
		}, content);
	};

	test('five underline styles render distinctly', async ({ page }) => {
		const CSI = '\x1b[';
		await setup(
			page,
			`${CSI}4:1mSOLID${CSI}24m ` +
				`${CSI}4:2mDOUBLE${CSI}24m ` +
				`${CSI}4:3mCURLY${CSI}24m ` +
				`${CSI}4:4mDOTTED${CSI}24m ` +
				`${CSI}4:5mDASHED${CSI}24m`,
		);
		const canvas = page.locator('canvas.alacritty-canvas');
		await expect(canvas).toHaveScreenshot('underline-styles.png', {
			maxDiffPixels: 400,
		});
	});

	test('cursor shape — beam', async ({ page }) => {
		// DECSCUSR 5 = blinking beam, rendered as a thin vertical bar.
		await setup(page, '\x1b[5 q');
		const canvas = page.locator('canvas.alacritty-canvas');
		await expect(canvas).toHaveScreenshot('cursor-beam.png', {
			maxDiffPixels: 200,
		});
	});

	test('cursor shape — underline', async ({ page }) => {
		await setup(page, '\x1b[3 q');
		const canvas = page.locator('canvas.alacritty-canvas');
		await expect(canvas).toHaveScreenshot('cursor-underline.png', {
			maxDiffPixels: 200,
		});
	});

	test('OSC 8 hyperlink renders with underline', async ({ page }) => {
		await setup(
			page,
			'plain \x1b]8;;https://example.com\x1b\\LINKED\x1b]8;;\x1b\\ plain',
		);
		const canvas = page.locator('canvas.alacritty-canvas');
		await expect(canvas).toHaveScreenshot('osc8-underline.png', {
			maxDiffPixels: 400,
		});
	});

	test('SGR bold + italic + ANSI colours', async ({ page }) => {
		const CSI = '\x1b[';
		await setup(
			page,
			`${CSI}31mRED${CSI}0m ${CSI}32mGREEN${CSI}0m ${CSI}34mBLUE${CSI}0m ` +
				`${CSI}1mBOLD${CSI}0m ${CSI}3mITALIC${CSI}0m`,
		);
		const canvas = page.locator('canvas.alacritty-canvas');
		await expect(canvas).toHaveScreenshot('sgr-bold-italic-colors.png', {
			maxDiffPixels: 400,
		});
	});
});
