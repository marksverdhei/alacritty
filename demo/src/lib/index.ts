/**
 * Public entry point for the WASM terminal component library.
 *
 * Consumers import these names; everything else under `$lib/` is treated
 * as an implementation detail.
 *
 *   import { AlacrittyTerminal, CardTerminal, themes } from '@alacritty/web';
 */

export { default as AlacrittyTerminal } from './components/AlacrittyTerminal.svelte';
export { default as MinimalTerminal } from './components/chrome/MinimalTerminal.svelte';
export { default as CardTerminal } from './components/chrome/CardTerminal.svelte';
export { default as WindowTerminal } from './components/chrome/WindowTerminal.svelte';
export { default as BannerTerminal } from './components/chrome/BannerTerminal.svelte';
export { default as TabbedTerminal } from './components/chrome/TabbedTerminal.svelte';
export { default as DevConsoleTerminal } from './components/chrome/DevConsoleTerminal.svelte';
export { default as RibbonTerminal } from './components/chrome/RibbonTerminal.svelte';
export { default as SidebarTerminal } from './components/chrome/SidebarTerminal.svelte';

export { themes, themeList, resolveTheme, themePalette, type Theme } from './themes';
export {
	loadAlacrittyConfig,
	applyAlacrittyConfig,
	type AlacrittyConfig,
} from './alacritty-config';
export { mapKeyToBytes } from './key-mapping';
