<script>
	let { children } = $props();
</script>

<svelte:head>
	<title>Alacritty Web - GPU-accelerated terminal in your browser</title>
	<meta name="description" content="Alacritty Web brings the fastest terminal emulator to your browser via WebAssembly." />
</svelte:head>

{@render children()}

<style>
	:global(*) {
		margin: 0;
		padding: 0;
		box-sizing: border-box;
	}

	:global(body) {
		background: #111317;
		color: #c5c8c6;
		font-family: system-ui, -apple-system, 'Segoe UI', sans-serif;
		min-height: 100vh;
	}

	:global(a) {
		color: #81a2be;
		text-decoration: none;
	}

	:global(a:hover) {
		text-decoration: underline;
	}
</style>
