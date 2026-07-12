<!--
	Sidebar chrome — a left rail with session/file icons, terminal on the
	right. Looks like an embedded IDE terminal panel.
-->
<script lang="ts">
	import AlacrittyTerminal from '../AlacrittyTerminal.svelte';
	import { resolveTheme, type Theme } from '$lib/themes';

	let {
		themeName = 'tokyo-night',
		entries = ['shell', 'build', 'test', 'lint'],
		activeEntry = 'shell',
		height = '320px',
		wsUrl = undefined as string | undefined,
		wsToken = undefined as string | undefined,
		onTerminalReady = undefined as ((t: any) => void) | undefined,
		onInput = undefined as ((b: Uint8Array) => void) | undefined,
	} = $props();

	let resolved: Theme = $derived(resolveTheme(themeName));
	let active = $state('shell');
	$effect(() => {
		active = activeEntry;
	});
</script>

<div class="side" style="--bg: {resolved.background}; --fg: {resolved.foreground}; --rail: {resolved.bright_black}; --accent: {resolved.cyan};">
	<aside class="rail" style="height: {height};">
		{#each entries as e}
			<button class="rail-btn" class:active={e === active} onclick={() => (active = e)}>
				<span class="bullet" style="background: {e === active ? resolved.cyan : 'transparent'}"></span>
				{e}
			</button>
		{/each}
	</aside>
	<div class="body" style="height: {height};">
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
	.side {
		display: grid;
		grid-template-columns: 140px 1fr;
		background: var(--bg);
		border-radius: 8px;
		overflow: hidden;
		border: 1px solid var(--rail);
	}
	.rail {
		background: color-mix(in srgb, var(--bg) 88%, white 12%);
		border-right: 1px solid var(--rail);
		padding: 8px 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.rail-btn {
		background: transparent;
		color: var(--fg);
		border: 0;
		padding: 6px 12px;
		text-align: left;
		display: flex;
		align-items: center;
		gap: 8px;
		font-family: ui-monospace, Menlo, monospace;
		font-size: 0.78rem;
		opacity: 0.7;
		cursor: pointer;
	}
	.rail-btn:hover { opacity: 0.95; }
	.rail-btn.active {
		opacity: 1;
		background: color-mix(in srgb, var(--accent) 16%, transparent);
	}
	.bullet {
		width: 6px; height: 6px;
		border-radius: 50%;
	}
	.body :global(.terminal-wrapper) {
		border: 0;
		background: transparent;
		height: 100%;
	}
	.body :global(.terminal-status) { display: none; }
</style>
