<!--
	Banner chrome — a coloured ribbon on top with a label, terminal below.
	Useful for "build output", "logs", "console" kinds of contexts where you
	want the role of the terminal to be visible at a glance.
-->
<script lang="ts">
	import AlacrittyTerminal from '../AlacrittyTerminal.svelte';
	import { resolveTheme, type Theme } from '$lib/themes';

	let {
		themeName = 'catppuccin-mocha',
		label = 'LOGS',
		accent = undefined as string | undefined,
		height = '300px',
		wsUrl = undefined as string | undefined,
		onTerminalReady = undefined as ((t: any) => void) | undefined,
		onInput = undefined as ((b: Uint8Array) => void) | undefined,
	} = $props();

	let resolved: Theme = $state(resolveTheme(themeName));
	let band = $derived(accent ?? resolved.green);
</script>

<div class="banner" style="--bg: {resolved.background}; --fg: {resolved.foreground};">
	<header class="band" style="background: {band};">
		<span class="label">{label}</span>
		<span class="theme">{resolved.name}</span>
	</header>
	<div class="body" style="height: {height};">
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
	.banner {
		border-radius: 8px;
		overflow: hidden;
		background: var(--bg);
	}
	.band {
		display: flex;
		justify-content: space-between;
		align-items: center;
		padding: 6px 14px;
		color: #111;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 0.72rem;
		font-weight: 700;
		letter-spacing: 0.08em;
	}
	.theme {
		font-weight: 400;
		opacity: 0.7;
		font-size: 0.7rem;
	}
	.body :global(.terminal-wrapper) {
		border: 0;
		background: transparent;
		height: 100%;
	}
	.body :global(.terminal-status) {
		display: none;
	}
</style>
