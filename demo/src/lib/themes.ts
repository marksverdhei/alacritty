/**
 * Built-in terminal themes for the WASM terminal component.
 *
 * Each theme is a flat palette object with the same keys consumed by the
 * wasm-side `set_palette` API — `background`, `foreground`, `cursor`,
 * the eight named ANSI colours, and their `bright_*` variants.
 *
 * Consumers can pick a theme by name with `<AlacrittyTerminal themeName="...">`
 * or splice their own into `themes` from outside the library.
 */

export interface Theme {
	name: string;
	id: string;
	mode: 'dark' | 'light';
	background: string;
	foreground: string;
	cursor: string;
	black: string;
	red: string;
	green: string;
	yellow: string;
	blue: string;
	magenta: string;
	cyan: string;
	white: string;
	bright_black: string;
	bright_red: string;
	bright_green: string;
	bright_yellow: string;
	bright_blue: string;
	bright_magenta: string;
	bright_cyan: string;
	bright_white: string;
}

export const themes: Record<string, Theme> = {
	'catppuccin-mocha': {
		name: 'Catppuccin Mocha', id: 'catppuccin-mocha', mode: 'dark',
		background: '#1e1e2e', foreground: '#cdd6f4', cursor: '#f5e0dc',
		black: '#45475a', red: '#f38ba8', green: '#a6e3a1', yellow: '#f9e2af',
		blue: '#89b4fa', magenta: '#f5c2e7', cyan: '#94e2d5', white: '#bac2de',
		bright_black: '#585b70', bright_red: '#f38ba8', bright_green: '#a6e3a1',
		bright_yellow: '#f9e2af', bright_blue: '#89b4fa', bright_magenta: '#f5c2e7',
		bright_cyan: '#94e2d5', bright_white: '#a6adc8',
	},
	'tokyo-night': {
		name: 'Tokyo Night', id: 'tokyo-night', mode: 'dark',
		background: '#1a1b26', foreground: '#a9b1d6', cursor: '#c0caf5',
		black: '#15161e', red: '#f7768e', green: '#9ece6a', yellow: '#e0af68',
		blue: '#7aa2f7', magenta: '#bb9af7', cyan: '#7dcfff', white: '#a9b1d6',
		bright_black: '#414868', bright_red: '#f7768e', bright_green: '#9ece6a',
		bright_yellow: '#e0af68', bright_blue: '#7aa2f7', bright_magenta: '#bb9af7',
		bright_cyan: '#7dcfff', bright_white: '#c0caf5',
	},
	'tomorrow-night': {
		name: 'Tomorrow Night', id: 'tomorrow-night', mode: 'dark',
		background: '#1d1f21', foreground: '#c5c8c6', cursor: '#c5c8c6',
		black: '#1d1f21', red: '#cc6666', green: '#b5bd68', yellow: '#f0c674',
		blue: '#81a2be', magenta: '#b294bb', cyan: '#8abeb7', white: '#c5c8c6',
		bright_black: '#969896', bright_red: '#cc6666', bright_green: '#b5bd68',
		bright_yellow: '#f0c674', bright_blue: '#81a2be', bright_magenta: '#b294bb',
		bright_cyan: '#8abeb7', bright_white: '#ffffff',
	},
	'dracula': {
		name: 'Dracula', id: 'dracula', mode: 'dark',
		background: '#282a36', foreground: '#f8f8f2', cursor: '#f8f8f0',
		black: '#000000', red: '#ff5555', green: '#50fa7b', yellow: '#f1fa8c',
		blue: '#bd93f9', magenta: '#ff79c6', cyan: '#8be9fd', white: '#bfbfbf',
		bright_black: '#4d4d4d', bright_red: '#ff6e67', bright_green: '#5af78e',
		bright_yellow: '#f4f99d', bright_blue: '#caa9fa', bright_magenta: '#ff92d0',
		bright_cyan: '#9aedfe', bright_white: '#e6e6e6',
	},
	'nord': {
		name: 'Nord', id: 'nord', mode: 'dark',
		background: '#2e3440', foreground: '#d8dee9', cursor: '#d8dee9',
		black: '#3b4252', red: '#bf616a', green: '#a3be8c', yellow: '#ebcb8b',
		blue: '#81a1c1', magenta: '#b48ead', cyan: '#88c0d0', white: '#e5e9f0',
		bright_black: '#4c566a', bright_red: '#bf616a', bright_green: '#a3be8c',
		bright_yellow: '#ebcb8b', bright_blue: '#81a1c1', bright_magenta: '#b48ead',
		bright_cyan: '#8fbcbb', bright_white: '#eceff4',
	},
	'gruvbox-dark': {
		name: 'Gruvbox Dark', id: 'gruvbox-dark', mode: 'dark',
		background: '#282828', foreground: '#ebdbb2', cursor: '#ebdbb2',
		black: '#282828', red: '#cc241d', green: '#98971a', yellow: '#d79921',
		blue: '#458588', magenta: '#b16286', cyan: '#689d6a', white: '#a89984',
		bright_black: '#928374', bright_red: '#fb4934', bright_green: '#b8bb26',
		bright_yellow: '#fabd2f', bright_blue: '#83a598', bright_magenta: '#d3869b',
		bright_cyan: '#8ec07c', bright_white: '#ebdbb2',
	},
	'solarized-dark': {
		name: 'Solarized Dark', id: 'solarized-dark', mode: 'dark',
		background: '#002b36', foreground: '#839496', cursor: '#93a1a1',
		black: '#073642', red: '#dc322f', green: '#859900', yellow: '#b58900',
		blue: '#268bd2', magenta: '#d33682', cyan: '#2aa198', white: '#eee8d5',
		bright_black: '#586e75', bright_red: '#cb4b16', bright_green: '#586e75',
		bright_yellow: '#657b83', bright_blue: '#839496', bright_magenta: '#6c71c4',
		bright_cyan: '#93a1a1', bright_white: '#fdf6e3',
	},
	'solarized-light': {
		name: 'Solarized Light', id: 'solarized-light', mode: 'light',
		background: '#fdf6e3', foreground: '#657b83', cursor: '#586e75',
		black: '#073642', red: '#dc322f', green: '#859900', yellow: '#b58900',
		blue: '#268bd2', magenta: '#d33682', cyan: '#2aa198', white: '#eee8d5',
		bright_black: '#002b36', bright_red: '#cb4b16', bright_green: '#586e75',
		bright_yellow: '#657b83', bright_blue: '#839496', bright_magenta: '#6c71c4',
		bright_cyan: '#93a1a1', bright_white: '#fdf6e3',
	},
	'one-dark': {
		name: 'One Dark', id: 'one-dark', mode: 'dark',
		background: '#282c34', foreground: '#abb2bf', cursor: '#abb2bf',
		black: '#3f4451', red: '#e06c75', green: '#98c379', yellow: '#e5c07b',
		blue: '#61afef', magenta: '#c678dd', cyan: '#56b6c2', white: '#abb2bf',
		bright_black: '#4f5666', bright_red: '#be5046', bright_green: '#98c379',
		bright_yellow: '#d19a66', bright_blue: '#61afef', bright_magenta: '#c678dd',
		bright_cyan: '#56b6c2', bright_white: '#e6e6e6',
	},
};

/** Convert a Theme into the palette object expected by `set_palette`. */
export function themePalette(t: Theme): Record<string, string> {
	const { name: _n, id: _i, mode: _m, ...rest } = t;
	return rest as unknown as Record<string, string>;
}

/** Resolve a theme by id, falling back to Catppuccin Mocha. */
export function resolveTheme(id: string | undefined | null): Theme {
	if (id && themes[id]) return themes[id];
	return themes['catppuccin-mocha'];
}

export const themeList: Theme[] = Object.values(themes);
