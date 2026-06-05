import { test, expect } from '@playwright/test';

/**
 * Unit-style coverage for `mapKeyToBytes` — runs inside the dev page so we
 * can import the actual module via Vite. Vitest isn't set up in this repo
 * and the keymap is pure (no DOM beyond `KeyboardEvent` shape), so dispatching
 * synthetic events through `page.evaluate` is the fastest path to coverage.
 */
test.describe('mapKeyToBytes', () => {
	test('encodes arrows with DECCKM and modifier matrix', async ({ page }) => {
		await page.goto('/');

		// Import the actual module from the Vite graph.
		const results = await page.evaluate(async () => {
			// @ts-expect-error — Vite serves .ts modules at this absolute URL
			// at runtime; the static-path resolver in tsc/svelte-check can't.
			const mod = await import('/src/lib/key-mapping.ts');
			const map = (mod as any).mapKeyToBytes as (e: any, modes?: number) => Uint8Array | null;

			// Helper: build a KeyboardEvent-shaped object.
			const ev = (key: string, mods: Partial<KeyboardEvent> = {}) => ({
				key,
				ctrlKey: !!mods.ctrlKey,
				altKey: !!mods.altKey,
				shiftKey: !!mods.shiftKey,
				metaKey: !!mods.metaKey,
			});
			const dec = (a: Uint8Array | null) =>
				a ? Array.from(a).map((b) => b.toString(16).padStart(2, '0')).join(' ') : null;

			return {
				// Plain arrow up — normal cursor mode.
				arrowUpPlain: dec(map(ev('ArrowUp'), 0)),
				// Same key with APP_CURSOR (DECCKM) on → SS3 form.
				arrowUpAppCursor: dec(map(ev('ArrowUp'), 1)),
				// Modifier overrides app-cursor: always CSI 1;mod form.
				shiftArrowUp: dec(map(ev('ArrowUp', { shiftKey: true }), 1)),
				ctrlArrowRight: dec(map(ev('ArrowRight', { ctrlKey: true }), 0)),
				// Shift+Tab → ESC [ Z.
				shiftTab: dec(map(ev('Tab', { shiftKey: true }), 0)),
				// Modified F-keys.
				ctrlF5: dec(map(ev('F5', { ctrlKey: true }), 0)),
				shiftF1: dec(map(ev('F1', { shiftKey: true }), 0)),
				// Modified PageUp.
				ctrlPageUp: dec(map(ev('PageUp', { ctrlKey: true }), 0)),
				// Home with app-cursor.
				homeAppCursor: dec(map(ev('Home'), 1)),
				// Ctrl+C still emits ^C, not modified-letter.
				ctrlC: dec(map(ev('c', { ctrlKey: true }), 0)),
			};
		});

		// Plain arrow up = ESC [ A.
		expect(results.arrowUpPlain).toBe('1b 5b 41');
		// App cursor mode = ESC O A.
		expect(results.arrowUpAppCursor).toBe('1b 4f 41');
		// Shift+Up = ESC [ 1 ; 2 A (mod=1+1=2). APP_CURSOR is ignored when modified.
		expect(results.shiftArrowUp).toBe('1b 5b 31 3b 32 41');
		// Ctrl+Right = ESC [ 1 ; 5 C (mod=1+4=5).
		expect(results.ctrlArrowRight).toBe('1b 5b 31 3b 35 43');
		// Shift+Tab = ESC [ Z.
		expect(results.shiftTab).toBe('1b 5b 5a');
		// Ctrl+F5 = ESC [ 15 ; 5 ~ → "1b 5b 31 35 3b 35 7e".
		expect(results.ctrlF5).toBe('1b 5b 31 35 3b 35 7e');
		// Shift+F1 = ESC [ 1 ; 2 P → "1b 5b 31 3b 32 50".
		expect(results.shiftF1).toBe('1b 5b 31 3b 32 50');
		// Ctrl+PageUp = ESC [ 5 ; 5 ~ → "1b 5b 35 3b 35 7e".
		expect(results.ctrlPageUp).toBe('1b 5b 35 3b 35 7e');
		// Home with app-cursor = ESC O H.
		expect(results.homeAppCursor).toBe('1b 4f 48');
		// Ctrl+C = 0x03.
		expect(results.ctrlC).toBe('03');
	});
});
