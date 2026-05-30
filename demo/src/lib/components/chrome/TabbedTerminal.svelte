<!--
	Tabbed chrome — Apple Terminal / VSCode-style tab strip on top. Only the
	active tab actually contains a live terminal; the inactive tabs are
	visual fillers that show the look of a multi-tab UI.
-->
<script lang="ts">
	import AlacrittyTerminal from '../AlacrittyTerminal.svelte';
	import { resolveTheme, type Theme } from '$lib/themes';

	let {
		themeName = 'catppuccin-mocha',
		tabs = ['shell', 'logs', 'tests'],
		activeIndex = 0,
		height = '320px',
		wsUrl = undefined as string | undefined,
		onTerminalReady = undefined as ((t: any) => void) | undefined,
		onInput = undefined as ((b: Uint8Array) => void) | undefined,
	} = $props();

	let resolved: Theme = $state(resolveTheme(themeName));
	let active = $state(activeIndex);
</script>

<div class="tabbed" style="--bg: {resolved.background}; --fg: {resolved.foreground}; --border: {resolved.bright_black}; --accent: {resolved.blue};">
	<div class="tab-strip">
		{#each tabs as t, i}
			<button class="tab" class:active={i === active} onclick={() => (active = i)}>
				<span class="tab-dot" style="background: {i === active ? resolved.green : resolved.bright_black}"></span>
				{t}
				<span class="tab-x">×</span>
			</button>
		{/each}
		<button class="tab-new">+</button>
	</div>
	<div class="tab-body" style="height: {height};">
		<AlacrittyTerminal
			{themeName}
			{wsUrl}
			{onTerminalReady}
			{onInput}
			onThemeResolved={(t) => (resolved = t)}
		/>
	</div>
</div>

<style>
	.tabbed {
		border: 1px solid var(--border);
		border-radius: 10px;
		overflow: hidden;
		background: var(--bg);
	}
	.tab-strip {
		display: flex;
		background: color-mix(in srgb, var(--bg) 88%, white 12%);
		border-bottom: 1px solid var(--border);
		padding: 4px 4px 0;
		gap: 2px;
	}
	.tab {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 6px 12px;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 0.78rem;
		color: var(--fg);
		opacity: 0.6;
		background: transparent;
		border: 0;
		border-radius: 6px 6px 0 0;
		cursor: pointer;
	}
	.tab.active {
		opacity: 1;
		background: var(--bg);
		box-shadow: 0 -2px 0 var(--accent) inset;
	}
	.tab-dot {
		width: 8px; height: 8px;
		border-radius: 50%;
	}
	.tab-x {
		opacity: 0.4;
		font-size: 1rem;
		line-height: 0;
	}
	.tab-new {
		background: transparent;
		border: 0;
		color: var(--fg);
		opacity: 0.5;
		padding: 6px 10px;
		cursor: pointer;
		font-size: 1rem;
	}
	.tab-new:hover { opacity: 0.9; }
	.tab-body :global(.terminal-wrapper) {
		border: 0;
		background: transparent;
		height: 100%;
	}
	.tab-body :global(.terminal-status) {
		display: none;
	}
</style>
