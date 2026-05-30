<!--
	DevConsole chrome — Chrome DevTools console look. Pill filters, a search
	box, and a "clear" button. Use it when the terminal is showing logs.
-->
<script lang="ts">
	import AlacrittyTerminal from '../AlacrittyTerminal.svelte';
	import { resolveTheme, type Theme } from '$lib/themes';

	let {
		themeName = 'one-dark',
		levels = ['all', 'errors', 'warnings', 'info'],
		activeLevel = 'all',
		height = '320px',
		onClear = undefined as (() => void) | undefined,
		wsUrl = undefined as string | undefined,
		onTerminalReady = undefined as ((t: any) => void) | undefined,
		onInput = undefined as ((b: Uint8Array) => void) | undefined,
	} = $props();

	let resolved: Theme = $state(resolveTheme(themeName));
	let level = $state(activeLevel);
</script>

<div class="dev" style="--bg: {resolved.background}; --fg: {resolved.foreground}; --border: {resolved.bright_black}; --accent: {resolved.blue}; --warn: {resolved.yellow}; --err: {resolved.red};">
	<div class="dev-bar">
		<button class="dev-clear" title="Clear console" onclick={onClear}>⌫</button>
		<div class="dev-filters">
			{#each levels as l}
				<button class="pill" class:active={l === level} onclick={() => (level = l)}>
					{l}
				</button>
			{/each}
		</div>
		<div class="dev-search">
			<span class="dev-search-icon">🔍</span>
			<input type="text" placeholder="filter" />
		</div>
	</div>
	<div class="dev-body" style="height: {height};">
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
	.dev {
		border: 1px solid var(--border);
		border-radius: 6px;
		overflow: hidden;
		background: var(--bg);
	}
	.dev-bar {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		padding: 6px 10px;
		background: color-mix(in srgb, var(--bg) 92%, white 8%);
		border-bottom: 1px solid var(--border);
		font-family: ui-sans-serif, system-ui, sans-serif;
		font-size: 0.76rem;
	}
	.dev-clear {
		background: transparent;
		color: var(--fg);
		border: 0;
		font-size: 1rem;
		cursor: pointer;
		opacity: 0.6;
		padding: 2px 6px;
	}
	.dev-clear:hover { opacity: 1; }
	.dev-filters {
		display: flex;
		gap: 4px;
	}
	.pill {
		background: transparent;
		color: var(--fg);
		border: 1px solid transparent;
		padding: 3px 10px;
		border-radius: 999px;
		font-size: 0.72rem;
		cursor: pointer;
		opacity: 0.55;
	}
	.pill:hover { opacity: 0.85; }
	.pill.active {
		opacity: 1;
		background: color-mix(in srgb, var(--accent) 18%, transparent);
		border-color: var(--accent);
		color: var(--accent);
	}
	.dev-search {
		display: flex;
		align-items: center;
		margin-left: auto;
		background: rgba(0, 0, 0, 0.2);
		border-radius: 4px;
		padding: 3px 8px;
		gap: 4px;
	}
	.dev-search-icon { opacity: 0.5; font-size: 0.72rem; }
	.dev-search input {
		background: transparent;
		border: 0;
		outline: 0;
		color: var(--fg);
		font-size: 0.72rem;
		font-family: ui-monospace, Menlo, monospace;
		width: 80px;
	}
	.dev-body :global(.terminal-wrapper) {
		border: 0;
		background: transparent;
		height: 100%;
	}
	.dev-body :global(.terminal-status) {
		display: none;
	}
</style>
