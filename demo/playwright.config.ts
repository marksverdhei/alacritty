import { defineConfig, devices } from '@playwright/test';

const demoPort = Number(process.env.PLAYWRIGHT_DEMO_PORT ?? 5174);
const ptyPort = 7681;
const demoOrigin = `http://127.0.0.1:${demoPort}`;

export default defineConfig({
	testDir: './tests',
	timeout: 60_000,
	expect: { timeout: 10_000 },
	fullyParallel: false,
	workers: 1,
	use: {
		baseURL: demoOrigin,
		headless: true,
		trace: 'retain-on-failure',
	},
	projects: [
		{ name: 'chromium', use: { ...devices['Desktop Chrome'] } },
	],
	webServer: [
		{
			command: `npm run dev -- --host 127.0.0.1 --port ${demoPort} --strictPort`,
			url: demoOrigin,
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
				`--port ${ptyPort} ` +
				`--allowed-origin ${demoOrigin} ` +
				`--allowed-origin http://localhost:${demoPort} ` +
				`--allowed-origin "http://[::1]:${demoPort}" ` +
				// Every test that loads /compare opens a fresh WebSocket; the
				// default 5-conn / 10s rate limit is sized for human use and
				// trips the suite. 0 disables the limit entirely.
				'--rate-limit-max 0',
			// The server speaks only WebSocket — Playwright's default HTTP
			// readiness check would fail. Use a TCP-level port check instead.
			port: ptyPort,
			reuseExistingServer: true,
			timeout: 15_000,
		},
	],
});
