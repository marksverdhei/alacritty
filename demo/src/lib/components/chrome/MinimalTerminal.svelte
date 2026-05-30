<!--
	Minimal chrome — just the terminal canvas with a theme-coloured border.
	Nothing on top, nothing below. Good for embedding inside another UI.
-->
<script lang="ts">
	import AlacrittyTerminal from '../AlacrittyTerminal.svelte';
	import { resolveTheme, type Theme } from '$lib/themes';

	let {
		themeName = 'catppuccin-mocha',
		height = '260px',
		wsUrl = undefined as string | undefined,
		onTerminalReady = undefined as ((t: any) => void) | undefined,
		onInput = undefined as ((b: Uint8Array) => void) | undefined,
	} = $props();

	let resolved: Theme = $state(resolveTheme(themeName));
</script>

<div class="minimal" style="height: {height}; background: {resolved.background}; border-color: {resolved.bright_black};">
	<AlacrittyTerminal
		{themeName}
		{wsUrl}
		{onTerminalReady}
		{onInput}
		onThemeResolved={(t) => (resolved = t)}
	/>
</div>

<style>
	.minimal {
		border: 1px solid transparent;
		border-radius: 6px;
		overflow: hidden;
	}
	.minimal :global(.terminal-wrapper) {
		border: 0;
		background: transparent;
		height: 100%;
	}
	.minimal :global(.terminal-status) {
		display: none;
	}
</style>
