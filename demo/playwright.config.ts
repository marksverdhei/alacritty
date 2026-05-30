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
	webServer: {
		command: 'npm run dev',
		url: 'http://localhost:5173',
		reuseExistingServer: true,
		timeout: 30_000,
	},
});
