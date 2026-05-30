<!--
	Ribbon chrome — a thin coloured stripe at the top with a path/branch
	label, plus a footer status bar (cwd, git branch, time). Vim/tmux feel.
-->
<script lang="ts">
	import AlacrittyTerminal from '../AlacrittyTerminal.svelte';
	import { resolveTheme, type Theme } from '$lib/themes';

	let {
		themeName = 'gruvbox-dark',
		path = '~/work/server',
		branch = 'main',
		height = '320px',
		mode = 'NORMAL',
		wsUrl = undefined as string | undefined,
		onTerminalReady = undefined as ((t: any) => void) | undefined,
		onInput = undefined as ((b: Uint8Array) => void) | undefined,
	} = $props();

	let resolved: Theme = $state(resolveTheme(themeName));
	let now = $state(new Date());
	if (typeof window !== 'undefined') {
		setInterval(() => (now = new Date()), 60_000);
	}
	let timeText = $derived(`${now.getHours().toString().padStart(2, '0')}:${now.getMinutes().toString().padStart(2, '0')}`);
</script>

<div class="ribbon" style="--bg: {resolved.background}; --fg: {resolved.foreground}; --accent: {resolved.green}; --muted: {resolved.bright_black};">
	<div class="strip" style="background: {resolved.green};">
		<span class="mode">{mode}</span>
		<span class="path">{path}</span>
		<span class="branch">{branch}</span>
	</div>
	<div class="body" style="height: {height};">
		<AlacrittyTerminal
			{themeName}
			{wsUrl}
			{onTerminalReady}
			{onInput}
			onThemeResolved={(t) => (resolved = t)}
		/>
	</div>
	<div class="footer" style="background: {resolved.bright_black};">
		<span class="cwd">{path}</span>
		<span class="sep">|</span>
		<span class="git" style="color: {resolved.yellow};"> {branch}</span>
		<span class="spacer"></span>
		<span class="clock">{timeText}</span>
	</div>
</div>

<style>
	.ribbon {
		display: flex;
		flex-direction: column;
		background: var(--bg);
		border-radius: 4px;
		overflow: hidden;
		font-family: ui-monospace, Menlo, monospace;
	}
	.strip {
		display: flex;
		align-items: center;
		gap: 1rem;
		padding: 4px 12px;
		color: #111;
		font-size: 0.74rem;
		font-weight: 600;
	}
	.mode {
		background: rgba(0, 0, 0, 0.18);
		padding: 1px 8px;
		border-radius: 2px;
		letter-spacing: 0.1em;
	}
	.path { flex: 1; }
	.branch::before { content: ' '; }
	.body :global(.terminal-wrapper) {
		border: 0;
		background: transparent;
		height: 100%;
	}
	.body :global(.terminal-status) { display: none; }
	.footer {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		padding: 4px 12px;
		color: var(--fg);
		font-size: 0.72rem;
	}
	.sep { opacity: 0.4; }
	.spacer { flex: 1; }
	.clock { opacity: 0.75; }
</style>
