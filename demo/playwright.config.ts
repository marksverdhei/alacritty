import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
	testDir: './tests',
	timeout: 60_000,
	expect: { timeout: 10_000 },
	fullyParallel: false,
	workers: 1,
	use: {
		baseURL: 'http://localhost:5173',
		headless: true,
		trace: 'retain-on-failure',
	},
	projects: [
		{ name: 'chromium', use: { ...devices['Desktop Chrome'] } },
	],
	webServer: [
		{
			command: 'npm run dev',
			url: 'http://localhost:5173',
			reuseExistingServer: true,
			timeout: 30_000,
		},
		// PTY server is only needed by live-pty.spec.ts. It's optional —
		// the rest of the suite feeds bytes directly into the wasm terminal
		// and doesn't need a real shell. Built via `cargo build --release
		// -p alacritty_pty_server` before this runs.
		{
			command:
				'../target/release/alacritty-pty-server ' +
				'--shell /bin/bash ' +
				'--allowed-origin http://localhost:5173 ' +
				'--allowed-origin "http://[::1]:5173" ' +
				// Every test that loads /compare opens a fresh WebSocket; the
				// default 5-conn / 10s rate limit is sized for human use and
				// trips the suite. 0 disables the limit entirely.
				'--rate-limit-max 0',
			// The server speaks only WebSocket — Playwright's default HTTP
			// readiness check would fail. Use a TCP-level port check instead.
			port: 7681,
			reuseExistingServer: true,
			timeout: 15_000,
		},
	],
});
