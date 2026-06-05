/**
 * Map a KeyboardEvent to the byte sequence a terminal expects on its PTY.
 * Returns null if the event should not produce input — e.g. modifier-only
 * keys, or unknown function keys.
 *
 * `modes` is the bitfield returned by `AlacrittyTerminal.keyboard_mode_bits()`:
 *   bit 0 = APP_CURSOR (DECCKM)  → arrows / Home / End use SS3 (`\eOA`) form
 *   bit 1 = APP_KEYPAD (DECPAM)  → not yet honored; reserved
 *   bit 2 = FOCUS_IN_OUT          → not part of key encoding
 *
 * Returning Uint8Array from a single place means both <AlacrittyTerminal>
 * and the /compare page send identical bytes for the same key, and bugfixes
 * here flow to every consumer.
 */
export function mapKeyToBytes(e: KeyboardEvent, modes: number = 0): Uint8Array | null {
	const appCursor = (modes & 1) !== 0;

	// xterm modifier encoding: 1 + shift + 2·alt + 4·ctrl + 8·meta. The result
	// is in 1..16; we emit a modified sequence whenever it's > 1.
	const mod =
		1 +
		(e.shiftKey ? 1 : 0) +
		((e.altKey || e.metaKey) ? 2 : 0) +
		(e.ctrlKey ? 4 : 0);
	const hasMod = mod > 1;

	// Ctrl + letter → control character (^A..^Z, plus a few extras).
	// Handled before the modified-arrow path because Ctrl+letter is shorter
	// and the CSI encoding doesn't apply to plain printable keys.
	if (e.ctrlKey && !e.altKey && !e.metaKey && e.key.length === 1) {
		const ch = e.key.toUpperCase();
		const c = ch.charCodeAt(0);
		// Letters A-Z map to 0x01-0x1A (^A..^Z).
		if (c >= 65 && c <= 90) return new Uint8Array([c - 64]);
		// Ctrl+Space → NUL, Ctrl+@ also NUL.
		if (ch === ' ' || ch === '@') return new Uint8Array([0]);
		// Ctrl+[ → ESC, Ctrl+\ → FS, Ctrl+] → GS, Ctrl+^ → RS, Ctrl+_ → US.
		switch (ch) {
			case '[': return new Uint8Array([0x1b]);
			case '\\': return new Uint8Array([0x1c]);
			case ']': return new Uint8Array([0x1d]);
			case '^': return new Uint8Array([0x1e]);
			case '_': return new Uint8Array([0x1f]);
			case '?': return new Uint8Array([0x7f]); // Ctrl+? → DEL
		}
	}

	// Helper: build `ESC [ 1 ; mod <letter>` (modified arrows / Home / End / F1-F4).
	const csiMod = (letter: string): Uint8Array => {
		const enc = new TextEncoder();
		return enc.encode(`\x1b[1;${mod}${letter}`);
	};
	// Helper: build `ESC [ n ; mod ~` (modified F5+, Insert/Delete/PageUp/PageDown).
	const csiTildeMod = (n: number): Uint8Array => {
		const enc = new TextEncoder();
		return enc.encode(`\x1b[${n};${mod}~`);
	};

	// Named keys. Arrows + Home + End respect APP_CURSOR when there's no
	// modifier; with a modifier xterm always uses the CSI 1;m form.
	switch (e.key) {
		case 'Enter': return new Uint8Array([13]);
		case 'Backspace': return new Uint8Array([127]);
		case 'Tab':
			// Shift+Tab → CSI Z. Other modifiers fall through to plain TAB
			// because no widely-supported encoding exists for Ctrl+Tab etc.
			if (e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey) {
				return new Uint8Array([0x1b, 0x5b, 0x5a]);
			}
			return new Uint8Array([9]);
		case 'Escape': return new Uint8Array([27]);

		case 'ArrowUp':
			if (hasMod) return csiMod('A');
			return appCursor
				? new Uint8Array([27, 0x4f, 65])
				: new Uint8Array([27, 91, 65]);
		case 'ArrowDown':
			if (hasMod) return csiMod('B');
			return appCursor
				? new Uint8Array([27, 0x4f, 66])
				: new Uint8Array([27, 91, 66]);
		case 'ArrowRight':
			if (hasMod) return csiMod('C');
			return appCursor
				? new Uint8Array([27, 0x4f, 67])
				: new Uint8Array([27, 91, 67]);
		case 'ArrowLeft':
			if (hasMod) return csiMod('D');
			return appCursor
				? new Uint8Array([27, 0x4f, 68])
				: new Uint8Array([27, 91, 68]);
		case 'Home':
			if (hasMod) return csiMod('H');
			return appCursor
				? new Uint8Array([27, 0x4f, 72])
				: new Uint8Array([27, 91, 72]);
		case 'End':
			if (hasMod) return csiMod('F');
			return appCursor
				? new Uint8Array([27, 0x4f, 70])
				: new Uint8Array([27, 91, 70]);

		case 'PageUp':   return hasMod ? csiTildeMod(5) : new Uint8Array([27, 91, 53, 126]);
		case 'PageDown': return hasMod ? csiTildeMod(6) : new Uint8Array([27, 91, 54, 126]);
		case 'Insert':   return hasMod ? csiTildeMod(2) : new Uint8Array([27, 91, 50, 126]);
		case 'Delete':   return hasMod ? csiTildeMod(3) : new Uint8Array([27, 91, 51, 126]);

		// F1-F4 use SS3 (ESC O) when unmodified, switch to CSI 1;mod{P..S} with mods.
		case 'F1': return hasMod ? csiMod('P') : new Uint8Array([27, 79, 80]);
		case 'F2': return hasMod ? csiMod('Q') : new Uint8Array([27, 79, 81]);
		case 'F3': return hasMod ? csiMod('R') : new Uint8Array([27, 79, 82]);
		case 'F4': return hasMod ? csiMod('S') : new Uint8Array([27, 79, 83]);

		case 'F5':  return hasMod ? csiTildeMod(15) : new Uint8Array([27, 91, 49, 53, 126]);
		case 'F6':  return hasMod ? csiTildeMod(17) : new Uint8Array([27, 91, 49, 55, 126]);
		case 'F7':  return hasMod ? csiTildeMod(18) : new Uint8Array([27, 91, 49, 56, 126]);
		case 'F8':  return hasMod ? csiTildeMod(19) : new Uint8Array([27, 91, 49, 57, 126]);
		case 'F9':  return hasMod ? csiTildeMod(20) : new Uint8Array([27, 91, 50, 48, 126]);
		case 'F10': return hasMod ? csiTildeMod(21) : new Uint8Array([27, 91, 50, 49, 126]);
		case 'F11': return hasMod ? csiTildeMod(23) : new Uint8Array([27, 91, 50, 51, 126]);
		case 'F12': return hasMod ? csiTildeMod(24) : new Uint8Array([27, 91, 50, 52, 126]);
	}

	if (e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) {
		return new TextEncoder().encode(e.key);
	}
	// Alt+letter sends ESC + letter, the standard "Meta" convention.
	if (e.altKey && e.key.length === 1) {
		return new Uint8Array([27, ...new TextEncoder().encode(e.key)]);
	}
	return null;
}
