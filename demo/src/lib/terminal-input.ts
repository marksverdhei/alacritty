/**
 * Small input-mapping helpers shared by every terminal canvas consumer
 * (AlacrittyTerminal.svelte and /compare). Both were redefining these
 * inline; drift between them caused real bugs (e.g. focus side-effects,
 * mouse-reporting parity gaps).
 */

/**
 * Minimal shape we need from the wasm terminal instance to translate a
 * mouse event into a grid cell. Typed as a structural interface rather
 * than the full AlacrittyTerminal type so test stubs and the runtime
 * wasm handle both work without casts.
 */
interface CellMetrics {
	cell_width: () => number;
	cell_height: () => number;
	cols: () => number;
	rows: () => number;
}

/** A grid cell hit by a mouse event, with which half of the cell was clicked. */
export interface CellHit {
	row: number;
	col: number;
	sideLeft: boolean;
}

/**
 * Translate a mouse event's clientX/Y into the grid cell it landed on.
 * Returns `null` when the terminal hasn't measured cell metrics yet
 * (typically right after mount, before the first render).
 */
export function cellFromMouseEvent(
	e: MouseEvent,
	canvas: HTMLCanvasElement,
	term: CellMetrics,
): CellHit | null {
	const cellW = term.cell_width();
	const cellH = term.cell_height();
	if (cellW <= 0 || cellH <= 0) return null;
	const rect = canvas.getBoundingClientRect();
	const x = e.clientX - rect.left;
	const y = e.clientY - rect.top;
	const col = Math.max(0, Math.min(term.cols() - 1, Math.floor(x / cellW)));
	const row = Math.max(0, Math.min(term.rows() - 1, Math.floor(y / cellH)));
	return { row, col, sideLeft: x - col * cellW < cellW / 2 };
}

/**
 * Pack DOM modifier flags into the bit format `report_mouse` expects:
 * bit 0 = shift, bit 1 = alt/meta, bit 2 = ctrl. Meta folds into alt
 * because xterm's mouse-reporting wire format has no separate meta bit.
 */
export function modBits(e: MouseEvent | WheelEvent): number {
	return (
		(e.shiftKey ? 1 : 0) |
		(e.altKey || e.metaKey ? 2 : 0) |
		(e.ctrlKey ? 4 : 0)
	);
}
