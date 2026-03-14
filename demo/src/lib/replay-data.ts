/**
 * Pre-recorded terminal session data for the demo/replay terminal.
 * Each frame is a tuple of [delay_ms, data_string].
 * The data includes ANSI escape sequences for colors and formatting.
 */

const ESC = '\x1b';
const CSI = `${ESC}[`;

// ANSI color helpers
const reset = `${CSI}0m`;
const bold = `${CSI}1m`;
const dim = `${CSI}2m`;
const green = `${CSI}32m`;
const yellow = `${CSI}33m`;
const blue = `${CSI}34m`;
const magenta = `${CSI}35m`;
const cyan = `${CSI}36m`;
const white = `${CSI}37m`;
const brightGreen = `${CSI}92m`;
const brightYellow = `${CSI}93m`;
const brightCyan = `${CSI}96m`;
const bgBlue = `${CSI}44m`;

export const replayFrames: [number, string][] = [
	// Clear screen and show prompt
	[500, `${CSI}2J${CSI}H`],
	[300, `${bold}${green}user@alacritty${reset}${white}:${bold}${blue}~${reset}$ `],

	// Type "neofetch" command
	[800, 'n'], [80, 'e'], [70, 'o'], [90, 'f'], [60, 'e'], [80, 't'], [70, 'c'], [60, 'h'],
	[400, '\r\n'],

	// Neofetch-style output
	[200, `\r\n`],
	[50, `  ${cyan}       /\\        ${reset}  ${bold}user@alacritty-web${reset}\r\n`],
	[50, `  ${cyan}      /  \\       ${reset}  ${dim}------------------${reset}\r\n`],
	[50, `  ${cyan}     /\\   \\      ${reset}  ${yellow}OS${reset}: Alacritty Web (WASM)\r\n`],
	[50, `  ${cyan}    /      \\     ${reset}  ${yellow}Host${reset}: Browser\r\n`],
	[50, `  ${cyan}   /   ,,   \\    ${reset}  ${yellow}Kernel${reset}: wasm32-unknown-unknown\r\n`],
	[50, `  ${cyan}  /   |  |   \\   ${reset}  ${yellow}Shell${reset}: alacritty-web 0.1.0\r\n`],
	[50, `  ${cyan} /_-''    ''-_\\  ${reset}  ${yellow}Renderer${reset}: Canvas2D\r\n`],
	[50, `  ${reset}                   ${yellow}Terminal${reset}: 80x24\r\n`],
	[50, `  ${reset}                   ${yellow}GPU${reset}: WebGPU (planned)\r\n`],
	[100, `\r\n`],
	[50, `  ${reset}  Colors:  `],
	[50, `${CSI}40m  ${CSI}41m  ${CSI}42m  ${CSI}43m  ${CSI}44m  ${CSI}45m  ${CSI}46m  ${CSI}47m  ${reset}\r\n`],
	[50, `  ${reset}          `],
	[50, `${CSI}100m  ${CSI}101m  ${CSI}102m  ${CSI}103m  ${CSI}104m  ${CSI}105m  ${CSI}106m  ${CSI}107m  ${reset}\r\n`],
	[200, `\r\n`],

	// New prompt
	[500, `${bold}${green}user@alacritty${reset}${white}:${bold}${blue}~${reset}$ `],

	// Type "cat welcome.txt"
	[1000, 'c'], [80, 'a'], [70, 't'], [60, ' '],
	[80, 'w'], [60, 'e'], [70, 'l'], [80, 'c'], [60, 'o'], [70, 'm'], [80, 'e'], [60, '.'], [70, 't'], [80, 'x'], [60, 't'],
	[400, '\r\n'],

	// Welcome message
	[200, `\r\n`],
	[50, `  ${bold}${brightCyan}Welcome to Alacritty Web!${reset}\r\n`],
	[50, `  ${dim}A GPU-accelerated terminal emulator${reset}\r\n`],
	[50, `  ${dim}running entirely in your browser.${reset}\r\n`],
	[100, `\r\n`],
	[50, `  ${brightGreen}Features:${reset}\r\n`],
	[50, `    ${green}+${reset} Full terminal emulation (VT100/xterm)\r\n`],
	[50, `    ${green}+${reset} ANSI color support (256 colors + truecolor)\r\n`],
	[50, `    ${green}+${reset} Canvas2D rendering (WebGPU planned)\r\n`],
	[50, `    ${green}+${reset} WebSocket PTY connection\r\n`],
	[50, `    ${green}+${reset} Resize support\r\n`],
	[50, `    ${green}+${reset} Built with Rust + wasm-bindgen\r\n`],
	[100, `\r\n`],

	// New prompt
	[500, `${bold}${green}user@alacritty${reset}${white}:${bold}${blue}~${reset}$ `],

	// Type a command showing truecolor
	[1200, 'e'], [50, 'c'], [50, 'h'], [50, 'o'], [50, ' '],
	[50, '"'], [50, 'T'], [50, 'r'], [50, 'u'], [50, 'e'], [50, 'c'], [50, 'o'], [50, 'l'], [50, 'o'], [50, 'r'], [50, '"'],
	[400, '\r\n'],
	[200, `\r\n`],

	// Gradient bar using 24-bit color
	[50, `  `],
	...(Array.from({ length: 40 }, (_, i): [number, string] => {
		const r = Math.round((i / 39) * 255);
		const g = Math.round(100 + ((39 - i) / 39) * 155);
		const b = Math.round(200 - (i / 39) * 100);
		return [20, `${ESC}[38;2;${r};${g};${b}m\u2588\u2588`];
	})),
	[50, `${reset}\r\n\r\n`],

	// Final prompt with blinking cursor effect
	[500, `${bold}${green}user@alacritty${reset}${white}:${bold}${blue}~${reset}$ `],
	[800, `${CSI}5m_${reset}`],
	[500, `${CSI}5m ${reset}`],
	[500, `${CSI}5m_${reset}`],
];
