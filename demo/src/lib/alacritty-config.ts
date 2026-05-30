/**
 * Minimal Alacritty config loader for the demo. Parses just the pieces we
 * can apply in the browser today (palette + font), follows a single import
 * level (good enough for omarchy's theme indirection), and silently ignores
 * everything else so unrelated keys like keybindings don't break us.
 */

export interface AlacrittyConfig {
	palette: Record<string, string>;
	fontFamily?: string;
	fontSize?: number;
}

interface ColorsSection {
	primary?: { background?: string; foreground?: string };
	cursor?: { cursor?: string; text?: string };
	normal?: Record<string, string>;
	bright?: Record<string, string>;
}

interface ParsedToml {
	general?: { import?: string[] };
	colors?: ColorsSection;
	font?: {
		normal?: { family?: string; style?: string };
		size?: number;
	};
}

/**
 * Hand-rolled TOML reader scoped to what alacritty configs typically use:
 * section headers, scalar string/number assignments, inline-table
 * assignments, and string-array assignments. No multi-line strings, no
 * comments inside values. Good enough for ~99% of real configs.
 */
function parseToml(src: string): ParsedToml {
	const out: any = {};
	let cursor: any = out;
	const path: string[] = [];

	const setPath = (segs: string[], leaf: any) => {
		let node = out;
		for (let i = 0; i < segs.length - 1; i++) {
			node[segs[i]] ??= {};
			node = node[segs[i]];
		}
		node[segs[segs.length - 1]] = leaf;
	};

	const valueOf = (raw: string): any => {
		const s = raw.trim();
		if (s.startsWith('"') && s.endsWith('"')) return s.slice(1, -1);
		if (s.startsWith("'") && s.endsWith("'")) return s.slice(1, -1);
		if (s.startsWith('[') && s.endsWith(']')) {
			return s.slice(1, -1).split(',').map(p => p.trim()).filter(Boolean)
				.map(p => (p.startsWith('"') ? p.slice(1, -1) : p));
		}
		if (s.startsWith('{') && s.endsWith('}')) {
			const obj: any = {};
			for (const pair of splitTopLevel(s.slice(1, -1), ',')) {
				const eq = pair.indexOf('=');
				if (eq === -1) continue;
				obj[pair.slice(0, eq).trim()] = valueOf(pair.slice(eq + 1));
			}
			return obj;
		}
		const n = Number(s);
		if (!Number.isNaN(n)) return n;
		return s;
	};

	const splitTopLevel = (s: string, sep: string): string[] => {
		const parts: string[] = [];
		let depth = 0, quote = '', start = 0;
		for (let i = 0; i < s.length; i++) {
			const c = s[i];
			if (quote) {
				if (c === quote) quote = '';
			} else if (c === '"' || c === "'") {
				quote = c;
			} else if (c === '{' || c === '[') {
				depth++;
			} else if (c === '}' || c === ']') {
				depth--;
			} else if (c === sep && depth === 0) {
				parts.push(s.slice(start, i));
				start = i + 1;
			}
		}
		parts.push(s.slice(start));
		return parts;
	};

	for (const rawLine of src.split('\n')) {
		const line = rawLine.replace(/(^|\s)#.*$/, '').trim();
		if (!line) continue;
		// Section header
		const sec = line.match(/^\[(.+)\]$/);
		if (sec) {
			path.length = 0;
			for (const seg of sec[1].split('.')) path.push(seg.trim());
			// Ensure the section exists
			let node = out;
			for (const seg of path) {
				node[seg] ??= {};
				node = node[seg];
			}
			cursor = node;
			continue;
		}
		// Assignment
		const eq = line.indexOf('=');
		if (eq === -1) continue;
		const key = line.slice(0, eq).trim();
		const val = valueOf(line.slice(eq + 1));
		// Dotted key (e.g. "padding.x = 14") under a section header.
		const segs = key.split('.').map(s => s.trim());
		if (path.length === 0) {
			setPath(segs, val);
		} else {
			setPath([...path, ...segs], val);
		}
	}
	return out;
}

/** Expand `~` in a path to the user's home — but in the browser we just
 *  strip everything before `omarchy/` (or similar) and serve from the
 *  hard-copied location. */
function rewriteImportPath(p: string): string {
	if (p.includes('omarchy/current/theme/alacritty.toml')) return '/alacritty-config/theme.toml';
	return p;
}

async function fetchToml(url: string): Promise<ParsedToml | null> {
	try {
		const r = await fetch(url);
		if (!r.ok) return null;
		return parseToml(await r.text());
	} catch {
		return null;
	}
}

/** Merge config B's known sections on top of A's. */
function mergeConfigs(a: ParsedToml, b: ParsedToml): ParsedToml {
	const out: ParsedToml = JSON.parse(JSON.stringify(a));
	if (b.colors) {
		out.colors = { ...(out.colors ?? {}), ...b.colors };
		if (b.colors.normal) {
			out.colors!.normal = { ...(out.colors?.normal ?? {}), ...b.colors.normal };
		}
		if (b.colors.bright) {
			out.colors!.bright = { ...(out.colors?.bright ?? {}), ...b.colors.bright };
		}
		if (b.colors.primary) {
			out.colors!.primary = { ...(out.colors?.primary ?? {}), ...b.colors.primary };
		}
		if (b.colors.cursor) {
			out.colors!.cursor = { ...(out.colors?.cursor ?? {}), ...b.colors.cursor };
		}
	}
	if (b.font) {
		out.font = { ...(out.font ?? {}), ...b.font };
	}
	return out;
}

/** Load the configured alacritty.toml + any single-level import. */
export async function loadAlacrittyConfig(baseUrl = '/alacritty-config/alacritty.toml'): Promise<AlacrittyConfig | null> {
	const base = await fetchToml(baseUrl);
	if (!base) return null;
	let merged = base;
	if (base.general?.import) {
		for (const importPath of base.general.import) {
			const url = rewriteImportPath(importPath);
			const inc = await fetchToml(url);
			if (inc) merged = mergeConfigs(inc, merged);
		}
	}

	const palette: Record<string, string> = {};
	const c = merged.colors;
	if (c?.primary?.background) palette.background = c.primary.background;
	if (c?.primary?.foreground) palette.foreground = c.primary.foreground;
	if (c?.cursor?.cursor) palette.cursor = c.cursor.cursor;
	for (const k of ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white']) {
		if (c?.normal?.[k]) palette[k] = c.normal[k];
		if (c?.bright?.[k]) palette['bright_' + k] = c.bright[k];
	}

	return {
		palette,
		fontFamily: merged.font?.normal?.family,
		fontSize: typeof merged.font?.size === 'number' ? merged.font.size : undefined,
	};
}

/** Apply a parsed config to a wasm AlacrittyTerminal instance. */
export function applyAlacrittyConfig(terminal: any, cfg: AlacrittyConfig) {
	if (cfg.fontFamily) {
		// Quote multi-word family names so they survive the CSS shorthand.
		const family = /[ '"]/.test(cfg.fontFamily) ? `'${cfg.fontFamily}'` : cfg.fontFamily;
		// Append a sane monospace fallback so the page still renders if
		// the user's font isn't installed in the browser's environment.
		terminal.set_font_family(
			`${family}, ui-monospace, 'SF Mono', Menlo, Consolas, monospace`,
		);
	}
	if (cfg.fontSize && cfg.fontSize > 0) {
		// Alacritty sizes are in points; CSS canvases work in px. 1pt ≈ 1.333px.
		terminal.set_font_size(cfg.fontSize * 1.333);
	}
	if (cfg.palette && Object.keys(cfg.palette).length > 0) {
		terminal.set_palette(cfg.palette);
	}
}
