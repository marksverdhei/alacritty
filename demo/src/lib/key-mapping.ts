/**
 * Map a KeyboardEvent to the byte sequence a terminal expects on its PTY.
 * Returns null if the event should not produce input — e.g. modifier-only
 * keys, or unknown function keys.
 *
 * Returning Uint8Array from a single place means both <AlacrittyTerminal>
 * and the /compare page send identical bytes for the same key, and bugfixes
 * here flow to every consumer.
 */
export function mapKeyToBytes(e: KeyboardEvent): Uint8Array | null {
	// Ctrl + letter → control character (^A..^Z, plus a few extras).
	if (e.ctrlKey && e.key.length === 1) {
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

	switch (e.key) {
		case 'Enter': return new Uint8Array([13]);
		case 'Backspace': return new Uint8Array([127]);
		case 'Tab': return new Uint8Array([9]);
		case 'Escape': return new Uint8Array([27]);
		case 'ArrowUp': return new Uint8Array([27, 91, 65]);
		case 'ArrowDown': return new Uint8Array([27, 91, 66]);
		case 'ArrowRight': return new Uint8Array([27, 91, 67]);
		case 'ArrowLeft': return new Uint8Array([27, 91, 68]);
		case 'Home': return new Uint8Array([27, 91, 72]);
		case 'End': return new Uint8Array([27, 91, 70]);
		case 'PageUp': return new Uint8Array([27, 91, 53, 126]);
		case 'PageDown': return new Uint8Array([27, 91, 54, 126]);
		case 'Insert': return new Uint8Array([27, 91, 50, 126]);
		case 'Delete': return new Uint8Array([27, 91, 51, 126]);
		case 'F1': return new Uint8Array([27, 79, 80]);
		case 'F2': return new Uint8Array([27, 79, 81]);
		case 'F3': return new Uint8Array([27, 79, 82]);
		case 'F4': return new Uint8Array([27, 79, 83]);
		case 'F5': return new Uint8Array([27, 91, 49, 53, 126]);
		case 'F6': return new Uint8Array([27, 91, 49, 55, 126]);
		case 'F7': return new Uint8Array([27, 91, 49, 56, 126]);
		case 'F8': return new Uint8Array([27, 91, 49, 57, 126]);
		case 'F9': return new Uint8Array([27, 91, 50, 48, 126]);
		case 'F10': return new Uint8Array([27, 91, 50, 49, 126]);
		case 'F11': return new Uint8Array([27, 91, 50, 51, 126]);
		case 'F12': return new Uint8Array([27, 91, 50, 52, 126]);
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
