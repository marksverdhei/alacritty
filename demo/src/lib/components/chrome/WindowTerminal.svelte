<!--
	Window chrome — macOS-style traffic-light buttons + window title.
	Looks great in screenshots and product pages.
-->
<script lang="ts">
	import AlacrittyTerminal from '../AlacrittyTerminal.svelte';
	import { resolveTheme, type Theme } from '$lib/themes';

	let {
		themeName = 'catppuccin-mocha',
		title = '~/projects',
		height = '320px',
		wsUrl = undefined as string | undefined,
		onTerminalReady = undefined as ((t: any) => void) | undefined,
		onInput = undefined as ((b: Uint8Array) => void) | undefined,
	} = $props();

	let resolved: Theme = $state(resolveTheme(themeName));
</script>

<div class="win" style="--bg: {resolved.background}; --fg: {resolved.foreground}; --chrome: {resolved.bright_black};">
	<header class="win-bar">
		<div class="lights">
			<span class="light close"></span>
			<span class="light min"></span>
			<span class="light max"></span>
		</div>
		<span class="win-title">{title}</span>
	</header>
	<div class="win-body" style="height: {height};">
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
	.win {
		border-radius: 10px;
		overflow: hidden;
		background: var(--bg);
		box-shadow: 0 8px 30px rgba(0, 0, 0, 0.35);
		border: 1px solid rgba(255, 255, 255, 0.06);
	}
	.win-bar {
		display: flex;
		align-items: center;
		padding: 8px 14px;
		background: var(--chrome);
		font-family: ui-monospace, Menlo, monospace;
		font-size: 0.78rem;
		color: var(--fg);
		position: relative;
	}
	.lights {
		display: flex;
		gap: 6px;
	}
	.light {
		width: 12px; height: 12px;
		border-radius: 50%;
		display: inline-block;
	}
	.light.close { background: #ff5f57; }
	.light.min { background: #febc2e; }
	.light.max { background: #28c840; }
	.win-title {
		position: absolute;
		left: 0; right: 0;
		text-align: center;
		opacity: 0.85;
		pointer-events: none;
	}
	.win-body :global(.terminal-wrapper) {
		border: 0;
		background: transparent;
		height: 100%;
	}
	.win-body :global(.terminal-status) {
		display: none;
	}
</style>
