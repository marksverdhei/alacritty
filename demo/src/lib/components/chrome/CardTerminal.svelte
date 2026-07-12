<!--
	Card chrome — title bar + canvas in a rounded card with a subtle border.
	Good as a default for docs / embedded examples.
-->
<script lang="ts">
	import AlacrittyTerminal from '../AlacrittyTerminal.svelte';
	import { resolveTheme, type Theme } from '$lib/themes';

	let {
		themeName = 'catppuccin-mocha',
		title = 'terminal',
		height = '300px',
		wsUrl = undefined as string | undefined,
		wsToken = undefined as string | undefined,
		onTerminalReady = undefined as ((t: any) => void) | undefined,
		onInput = undefined as ((b: Uint8Array) => void) | undefined,
	} = $props();

	let resolved: Theme = $derived(resolveTheme(themeName));
</script>

<div class="card" style="--bg: {resolved.background}; --fg: {resolved.foreground}; --border: {resolved.bright_black}; --accent: {resolved.blue};">
	<header class="card-head">
		<span class="dot" style="background: {resolved.green};"></span>
		<span class="title">{title}</span>
		<span class="theme-tag">{resolved.name}</span>
	</header>
	<div class="card-body" style="height: {height};">
		<AlacrittyTerminal
			{themeName}
			{wsUrl}
			{wsToken}
			{onTerminalReady}
			{onInput}
		/>
	</div>
</div>

<style>
	.card {
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: 10px;
		overflow: hidden;
		box-shadow: 0 4px 24px rgba(0, 0, 0, 0.25);
	}
	.card-head {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 8px 14px;
		background: rgba(255, 255, 255, 0.04);
		border-bottom: 1px solid var(--border);
		font-family: ui-monospace, Menlo, monospace;
		font-size: 0.78rem;
	}
	.dot {
		width: 9px; height: 9px; border-radius: 50%;
	}
	.title {
		color: var(--fg);
		flex: 1;
		opacity: 0.85;
	}
	.theme-tag {
		color: var(--accent);
		font-size: 0.7rem;
		opacity: 0.7;
	}
	.card-body :global(.terminal-wrapper) {
		border: 0;
		background: transparent;
		height: 100%;
	}
	.card-body :global(.terminal-status) {
		display: none;
	}
</style>
