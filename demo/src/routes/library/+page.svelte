<script lang="ts">
	import { themeList } from '$lib/themes';
	import MinimalTerminal from '$lib/components/chrome/MinimalTerminal.svelte';
	import CardTerminal from '$lib/components/chrome/CardTerminal.svelte';
	import WindowTerminal from '$lib/components/chrome/WindowTerminal.svelte';
	import BannerTerminal from '$lib/components/chrome/BannerTerminal.svelte';
	import TabbedTerminal from '$lib/components/chrome/TabbedTerminal.svelte';
	import DevConsoleTerminal from '$lib/components/chrome/DevConsoleTerminal.svelte';
	import RibbonTerminal from '$lib/components/chrome/RibbonTerminal.svelte';
	import SidebarTerminal from '$lib/components/chrome/SidebarTerminal.svelte';

	// Feed a deterministic, theme-agnostic snippet into each terminal so the
	// gallery shows visual differences in colour + chrome, not content.
	const sampleBytes = (() => {
		const CSI = '\x1b[';
		const lines = [
			`${CSI}1;32m➜${CSI}0m  ${CSI}1;34m~/projects${CSI}0m ${CSI}90m(main)${CSI}0m`,
			`$ npm run build`,
			``,
			`${CSI}90m> demo@0.1.0 build${CSI}0m`,
			`${CSI}90m> vite build${CSI}0m`,
			``,
			`${CSI}1;36mvite v6.0.0${CSI}0m ${CSI}90mbuilding for production...${CSI}0m`,
			`✓ ${CSI}32m37 modules transformed.${CSI}0m`,
			``,
			`dist/${CSI}1mindex.html${CSI}0m              ${CSI}33m   0.46 kB${CSI}0m`,
			`dist/assets/${CSI}1mapp-Df93.js${CSI}0m   ${CSI}33m  82.43 kB${CSI}0m │ gzip: ${CSI}32m 28.10 kB${CSI}0m`,
			`dist/assets/${CSI}1mwasm-9Hb2.wasm${CSI}0m ${CSI}33m 432.10 kB${CSI}0m`,
			``,
			`${CSI}32m✓${CSI}0m built in ${CSI}1m1.42s${CSI}0m`,
		];
		return new TextEncoder().encode(lines.join('\r\n') + '\r\n');
	})();

	const feedSample = (terminal: any) => {
		terminal.feed(sampleBytes);
	};

	type ChromeId = 'minimal' | 'card' | 'window' | 'banner' | 'tabbed' | 'devconsole' | 'ribbon' | 'sidebar';
	const chromeOptions: { id: ChromeId; label: string }[] = [
		{ id: 'minimal', label: 'Minimal' },
		{ id: 'card', label: 'Card' },
		{ id: 'window', label: 'Window' },
		{ id: 'banner', label: 'Banner' },
		{ id: 'tabbed', label: 'Tabbed' },
		{ id: 'devconsole', label: 'DevConsole' },
		{ id: 'ribbon', label: 'Ribbon' },
		{ id: 'sidebar', label: 'Sidebar' },
	];

	// Pair each chrome with a fitting theme for the showcase row.
	const showcase: { chrome: ChromeId; theme: string; props: Record<string, any> }[] = [
		{ chrome: 'minimal', theme: 'catppuccin-mocha', props: {} },
		{ chrome: 'card', theme: 'tomorrow-night', props: { title: 'build.log' } },
		{ chrome: 'window', theme: 'dracula', props: { title: '~/projects — npm run build' } },
		{ chrome: 'banner', theme: 'nord', props: { label: 'CI OUTPUT' } },
		{ chrome: 'tabbed', theme: 'one-dark', props: { tabs: ['shell', 'build', 'tests'] } },
		{ chrome: 'devconsole', theme: 'tokyo-night', props: {} },
		{ chrome: 'ribbon', theme: 'gruvbox-dark', props: { path: '~/code/api', branch: 'main' } },
		{ chrome: 'sidebar', theme: 'solarized-dark', props: { entries: ['shell', 'build', 'test'] } },
	];

	let selectedChrome = $state<ChromeId>('card');
</script>

<svelte:head>
	<title>Terminal component library</title>
</svelte:head>

<div class="page">
	<header class="hero">
		<h1>Terminal component library</h1>
		<p class="sub">
			Same wasm engine. Drop-in components, eight built-in themes, four
			chrome styles. Pick a chrome style below to see the matrix.
		</p>
		<div class="chrome-picker">
			{#each chromeOptions as opt}
				<button
					class="chrome-btn"
					class:active={selectedChrome === opt.id}
					onclick={() => (selectedChrome = opt.id)}
				>
					{opt.label}
				</button>
			{/each}
		</div>
	</header>

	<section class="usage">
		<pre><code>{`import { CardTerminal } from '@alacritty/web';

<CardTerminal themeName="${themeList[0].id}" title="build.log" />`}</code></pre>
	</section>

	<section class="section">
		<h2 class="section-h">Chrome variants</h2>
		<p class="section-sub">
			Each card wraps the same wasm terminal in a different outer shape.
			Paired with a different theme so you can see how they read in context.
		</p>
		<div class="grid">
			{#each showcase as item}
				<div class="cell">
					<div class="cell-head">
						<div class="theme-meta">
							<div class="theme-name">{chromeOptions.find(c => c.id === item.chrome)?.label}</div>
							<div class="theme-id">{item.theme}</div>
						</div>
					</div>
					{#if item.chrome === 'minimal'}
						<MinimalTerminal themeName={item.theme} height="200px" onTerminalReady={feedSample} />
					{:else if item.chrome === 'card'}
						<CardTerminal themeName={item.theme} {...item.props} height="200px" onTerminalReady={feedSample} />
					{:else if item.chrome === 'window'}
						<WindowTerminal themeName={item.theme} {...item.props} height="200px" onTerminalReady={feedSample} />
					{:else if item.chrome === 'banner'}
						<BannerTerminal themeName={item.theme} {...item.props} height="200px" onTerminalReady={feedSample} />
					{:else if item.chrome === 'tabbed'}
						<TabbedTerminal themeName={item.theme} {...item.props} height="200px" onTerminalReady={feedSample} />
					{:else if item.chrome === 'devconsole'}
						<DevConsoleTerminal themeName={item.theme} {...item.props} height="200px" onTerminalReady={feedSample} />
					{:else if item.chrome === 'ribbon'}
						<RibbonTerminal themeName={item.theme} {...item.props} height="200px" onTerminalReady={feedSample} />
					{:else if item.chrome === 'sidebar'}
						<SidebarTerminal themeName={item.theme} {...item.props} height="200px" onTerminalReady={feedSample} />
					{/if}
				</div>
			{/each}
		</div>
	</section>

	<section class="section">
		<h2 class="section-h">Theme gallery</h2>
		<p class="section-sub">
			Same chrome ({chromeOptions.find(c => c.id === selectedChrome)?.label}),
			every built-in theme. Switch the chrome above to recompose.
		</p>
		<div class="grid">
			{#each themeList as theme (theme.id + selectedChrome)}
				<div class="cell">
					<div class="cell-head">
						<span class="theme-swatch" style="background: {theme.background}; border-color: {theme.bright_black};">
							<span class="swatch-row">
								<span style="background: {theme.red}"></span>
								<span style="background: {theme.green}"></span>
								<span style="background: {theme.yellow}"></span>
								<span style="background: {theme.blue}"></span>
								<span style="background: {theme.magenta}"></span>
								<span style="background: {theme.cyan}"></span>
							</span>
						</span>
						<div class="theme-meta">
							<div class="theme-name">{theme.name}</div>
							<div class="theme-id">{theme.id}</div>
						</div>
					</div>

					{#if selectedChrome === 'minimal'}
						<MinimalTerminal themeName={theme.id} height="200px" onTerminalReady={feedSample} />
					{:else if selectedChrome === 'card'}
						<CardTerminal themeName={theme.id} title="build.log" height="200px" onTerminalReady={feedSample} />
					{:else if selectedChrome === 'window'}
						<WindowTerminal themeName={theme.id} title="~/projects" height="200px" onTerminalReady={feedSample} />
					{:else if selectedChrome === 'banner'}
						<BannerTerminal themeName={theme.id} label="BUILD" height="200px" onTerminalReady={feedSample} />
					{:else if selectedChrome === 'tabbed'}
						<TabbedTerminal themeName={theme.id} height="200px" onTerminalReady={feedSample} />
					{:else if selectedChrome === 'devconsole'}
						<DevConsoleTerminal themeName={theme.id} height="200px" onTerminalReady={feedSample} />
					{:else if selectedChrome === 'ribbon'}
						<RibbonTerminal themeName={theme.id} height="200px" onTerminalReady={feedSample} />
					{:else if selectedChrome === 'sidebar'}
						<SidebarTerminal themeName={theme.id} height="200px" onTerminalReady={feedSample} />
					{/if}
				</div>
			{/each}
		</div>
	</section>

	<footer class="footer">
		<p>
			<a href="/">← Home</a> · <a href="/compare">Compare with xterm.js</a>
		</p>
	</footer>
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
		margin: 0 0 0.5rem;
		color: #e0e0e0;
	}
	.sub {
		color: #969896;
		font-size: 0.9rem;
		margin: 0 0 1rem;
	}
	.chrome-picker {
		display: inline-flex;
		gap: 4px;
		background: #1f2226;
		padding: 4px;
		border-radius: 8px;
		border: 1px solid #2a2d33;
	}
	.chrome-btn {
		background: transparent;
		color: #969896;
		border: 0;
		padding: 6px 14px;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 0.82rem;
		cursor: pointer;
		border-radius: 6px;
		transition: background 0.12s, color 0.12s;
	}
	.chrome-btn:hover {
		color: #e0e0e0;
	}
	.chrome-btn.active {
		background: #2a2d33;
		color: #e0e0e0;
	}

	.usage {
		max-width: 720px;
		margin: 0 auto 1.5rem;
	}
	.usage pre {
		background: #1a1c20;
		border: 1px solid #2a2d33;
		border-radius: 8px;
		padding: 0.9rem 1.2rem;
		color: #cdd6f4;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 0.78rem;
		overflow-x: auto;
		margin: 0;
	}

	.section {
		margin-bottom: 2.5rem;
	}
	.section-h {
		font-size: 1.05rem;
		color: #e0e0e0;
		margin: 0 0 0.25rem;
	}
	.section-sub {
		color: #969896;
		font-size: 0.85rem;
		margin: 0 0 1rem;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(420px, 1fr));
		gap: 1.2rem;
	}
	.cell {
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
	}
	.cell-head {
		display: flex;
		align-items: center;
		gap: 0.6rem;
	}
	.theme-swatch {
		display: inline-flex;
		flex-direction: column;
		padding: 4px;
		border: 1px solid;
		border-radius: 4px;
	}
	.swatch-row {
		display: flex;
		gap: 2px;
	}
	.swatch-row span {
		width: 9px;
		height: 9px;
		border-radius: 2px;
		display: block;
	}
	.theme-meta {
		display: flex;
		flex-direction: column;
		line-height: 1.1;
	}
	.theme-name {
		font-size: 0.85rem;
		color: #e0e0e0;
		font-weight: 600;
	}
	.theme-id {
		font-size: 0.7rem;
		color: #707782;
		font-family: ui-monospace, Menlo, monospace;
	}

	.footer {
		margin-top: 2.5rem;
		text-align: center;
		color: #6f7782;
	}
	.footer a {
		color: #81a2be;
		text-decoration: none;
	}
	.footer a:hover { color: #b5bd68; }
</style>
