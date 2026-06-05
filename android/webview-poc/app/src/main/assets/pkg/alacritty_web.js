let wasm;

let cachedUint8ArrayMemory0 = null;

function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });

cachedTextDecoder.decode();

const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(ptr, len);
}

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_export_2.set(idx, obj);
    return idx;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let WASM_VECTOR_LEN = 0;

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    }
}

function passStringToWasm0(arg, malloc, realloc) {

    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }

    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

let cachedDataViewMemory0 = null;

function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches && builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(
state => {
    wasm.__wbindgen_export_6.get(state.dtor)(state.a, state.b);
}
);

function makeMutClosure(arg0, arg1, dtor, f) {
    const state = { a: arg0, b: arg1, cnt: 1, dtor };
    const real = (...args) => {

        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            if (--state.cnt === 0) {
                wasm.__wbindgen_export_6.get(state.dtor)(a, state.b);
                CLOSURE_DTORS.unregister(state);
            } else {
                state.a = a;
            }
        }
    };
    real.original = state;
    CLOSURE_DTORS.register(real, state, state);
    return real;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_export_2.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}
function __wbg_adapter_6(arg0, arg1) {
    wasm.wasm_bindgen__convert__closures_____invoke__h5baad699b3dcff72(arg0, arg1);
}

function __wbg_adapter_9(arg0, arg1, arg2) {
    wasm.closure129_externref_shim(arg0, arg1, arg2);
}

const __wbindgen_enum_BinaryType = ["blob", "arraybuffer"];

const AlacrittyTerminalFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_alacrittyterminal_free(ptr >>> 0, 1));
/**
 * The main Alacritty terminal component for the browser.
 */
export class AlacrittyTerminal {

    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        AlacrittyTerminalFinalization.unregister(this);
        return ptr;
    }

    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_alacrittyterminal_free(ptr, 0);
    }
    /**
     * Get cell width in pixels. Cached so this is safe to call re-entrantly
     * (e.g. from a ResizeObserver while a render borrow is active).
     * @returns {number}
     */
    cell_width() {
        const ret = wasm.alacrittyterminal_cell_width(this.__wbg_ptr);
        return ret;
    }
    /**
     * Disconnect from the PTY server.
     */
    disconnect() {
        wasm.alacrittyterminal_disconnect(this.__wbg_ptr);
    }
    /**
     * Get cell height in pixels. Cached -- see `cell_width` for why.
     * @returns {number}
     */
    cell_height() {
        const ret = wasm.alacrittyterminal_cell_height(this.__wbg_ptr);
        return ret;
    }
    /**
     * Set the focused state. Drives the solid-vs-hollow cursor shape.
     * @param {boolean} focused
     */
    set_focused(focused) {
        wasm.alacrittyterminal_set_focused(this.__wbg_ptr, focused);
    }
    /**
     * Apply a palette override taken from a user's alacritty config. Each
     * entry is a 7-char "#RRGGBB" string. Unknown keys are ignored. Any
     * missing key falls back to the built-in default. Accepts the keys:
     *   background, foreground, cursor,
     *   black, red, green, yellow, blue, magenta, cyan, white,
     *   bright_black, bright_red, bright_green, bright_yellow,
     *   bright_blue, bright_magenta, bright_cyan, bright_white.
     * @param {any} palette
     */
    set_palette(palette) {
        const ret = wasm.alacrittyterminal_set_palette(this.__wbg_ptr, palette);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Encode a mouse event into a PTY byte sequence following the active
     * terminal mouse-reporting mode (DECSET 1000/1002/1003/1006). Returns
     * `None` when no mouse reporting is enabled OR the event should not be
     * reported (e.g. a motion event when only click reporting is on).
     * The JS side calls this from mousedown/mouseup/mousemove/wheel; if it
     * gets bytes back it writes them to the PTY and skips its own default
     * behaviour (start selection, scroll).
     *
     * Parameters:
     * - `button`: canonical xterm button code. 0=left, 1=middle, 2=right,
     *   3=released (used by legacy non-SGR encoding), 64=wheel up, 65=wheel
     *   down. For "motion without button held" pass 3.
     * - `action`: 0=press, 1=release, 2=motion.
     * - `col`, `row`: zero-based grid coordinates inside the viewport.
     * - `mods`: bit0=shift, bit1=alt/meta, bit2=ctrl.
     * @param {number} button
     * @param {number} action
     * @param {number} col
     * @param {number} row
     * @param {number} mods
     * @returns {Uint8Array | undefined}
     */
    report_mouse(button, action, col, row, mods) {
        const ret = wasm.alacrittyterminal_report_mouse(this.__wbg_ptr, button, action, col, row, mods);
        let v1;
        if (ret[0] !== 0) {
            v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Wall time spent draining/parsing PTY bytes during the most recent
     * RAF that did work, in milliseconds. Zero when no data was processed.
     * @returns {number}
     */
    last_parse_ms() {
        const ret = wasm.alacrittyterminal_last_parse_ms(this.__wbg_ptr);
        return ret;
    }
    /**
     * Number of fed-but-not-yet-parsed bytes. JS polls this to know when a
     * feed() has been consumed by the next RAF.
     * @returns {number}
     */
    pending_bytes() {
        const ret = wasm.alacrittyterminal_pending_bytes(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Set the font size in pixels and trigger a re-render.
     * @param {number} size_px
     */
    set_font_size(size_px) {
        wasm.alacrittyterminal_set_font_size(this.__wbg_ptr, size_px);
    }
    /**
     * Wall time spent in the renderer (paint into canvas) during the most
     * recent RAF that did work, in milliseconds.
     * @returns {number}
     */
    last_render_ms() {
        const ret = wasm.alacrittyterminal_last_render_ms(this.__wbg_ptr);
        return ret;
    }
    /**
     * Back-compat alias for the older bench name. Returns 0 if no pending,
     * nonzero if pending — same semantics the benchmark cares about.
     * @returns {number}
     */
    pending_chunks() {
        const ret = wasm.alacrittyterminal_pending_chunks(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Select the entire line at the given viewport cell.
     * @param {number} row
     * @param {number} column
     */
    selection_line(row, column) {
        wasm.alacrittyterminal_selection_line(this.__wbg_ptr, row, column);
    }
    /**
     * Return the current selection as a string, or undefined if nothing is
     * selected. Useful for the JS side to implement copy-to-clipboard.
     * @returns {string | undefined}
     */
    selection_text() {
        const ret = wasm.alacrittyterminal_selection_text(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Select the word at the given viewport cell (semantic boundaries).
     * @param {number} row
     * @param {number} column
     */
    selection_word(row, column) {
        wasm.alacrittyterminal_selection_word(this.__wbg_ptr, row, column);
    }
    /**
     * Current state of the underlying WebSocket, mapped from `WebSocket.readyState`.
     * Returns 0 = connecting, 1 = open, 2 = closing, 3 = closed, -1 = no socket.
     * @returns {number}
     */
    ws_ready_state() {
        const ret = wasm.alacrittyterminal_ws_ready_state(this.__wbg_ptr);
        return ret;
    }
    /**
     * Clear any active selection.
     */
    selection_clear() {
        wasm.alacrittyterminal_selection_clear(this.__wbg_ptr);
    }
    /**
     * Start a selection at the given viewport cell. `row` counts from the top
     * of the visible area (0..rows). `side_left` selects whether the click
     * landed on the left or right half of the cell.
     * @param {number} row
     * @param {number} column
     * @param {boolean} side_left
     */
    selection_start(row, column, side_left) {
        wasm.alacrittyterminal_selection_start(this.__wbg_ptr, row, column, side_left);
    }
    /**
     * Set the font family and trigger a re-render.
     * @param {string} family
     */
    set_font_family(family) {
        const ptr0 = passStringToWasm0(family, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.alacrittyterminal_set_font_family(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Get the active renderer backend name ("wgpu" or "canvas2d").
     * @returns {string}
     */
    renderer_backend() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.alacrittyterminal_renderer_backend(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Jump the display viewport to the bottom (most recent output).
     */
    scroll_to_bottom() {
        wasm.alacrittyterminal_scroll_to_bottom(this.__wbg_ptr);
    }
    /**
     * Extend the active selection to the given viewport cell.
     * @param {number} row
     * @param {number} column
     * @param {boolean} side_left
     */
    selection_update(row, column, side_left) {
        wasm.alacrittyterminal_selection_update(this.__wbg_ptr, row, column, side_left);
    }
    /**
     * Resize the canvas backing store to match its CSS size. Call this
     * whenever the canvas element's size changes (e.g. from ResizeObserver).
     */
    sync_canvas_size() {
        wasm.alacrittyterminal_sync_canvas_size(this.__wbg_ptr);
    }
    /**
     * Packed keyboard-mode flags for the JS side to consult on each keystroke.
     * Bit 0 = APP_CURSOR (DECCKM) — arrows/Home/End use SS3 (`\eOA`) form
     * Bit 1 = APP_KEYPAD (DECPAM)
     * Bit 2 = FOCUS_IN_OUT (DECSET 1004) — host should emit `\e[I` / `\e[O`
     * @returns {number}
     */
    keyboard_mode_bits() {
        const ret = wasm.alacrittyterminal_keyboard_mode_bits(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Whether the terminal currently wants mouse events forwarded to the PTY.
     * JS reads this in mousedown to decide between starting a selection and
     * calling `report_mouse`.
     * @returns {boolean}
     */
    mouse_reporting_active() {
        const ret = wasm.alacrittyterminal_mouse_reporting_active(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Set the line height multiplier and trigger a re-render.
     * @param {number} multiplier
     */
    set_line_height_multiplier(multiplier) {
        wasm.alacrittyterminal_set_line_height_multiplier(this.__wbg_ptr, multiplier);
    }
    /**
     * Create a new terminal attached to the given canvas element.
     * @param {HTMLCanvasElement} canvas
     */
    constructor(canvas) {
        const ret = wasm.alacrittyterminal_new(canvas);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0] >>> 0;
        AlacrittyTerminalFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Get the number of columns in the terminal grid.
     * @returns {number}
     */
    cols() {
        const ret = wasm.alacrittyterminal_cols(this.__wbg_ptr);
        return ret;
    }
    /**
     * Feed data directly into the terminal (for replay/local input, no PTY).
     * Appended to a flat buffer so a burst of small calls turns into a single
     * `parser.advance()` invocation in the next RAF.
     * @param {Uint8Array} data
     */
    feed(data) {
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.alacrittyterminal_feed(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Get the number of rows in the terminal grid.
     * @returns {number}
     */
    rows() {
        const ret = wasm.alacrittyterminal_rows(this.__wbg_ptr);
        return ret;
    }
    /**
     * Paste text into the PTY. Wraps the text with bracketed-paste markers
     * when the terminal has requested that mode, so shells like bash can
     * distinguish pasted bytes from typed bytes.
     * @param {string} text
     */
    paste(text) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.alacrittyterminal_paste(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Write data to the PTY (send input). Also snaps the viewport back to
     * the bottom and clears any active selection -- matches native Alacritty.
     * @param {Uint8Array} data
     */
    write(data) {
        const ptr0 = passArray8ToWasm0(data, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.alacrittyterminal_write(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Send a resize message to the server.
     * @param {number} cols
     * @param {number} rows
     */
    resize(cols, rows) {
        wasm.alacrittyterminal_resize(this.__wbg_ptr, cols, rows);
    }
    /**
     * Scroll the display viewport by `delta` lines. Positive scrolls into
     * scrollback (towards older output), negative scrolls towards the bottom.
     * @param {number} delta
     */
    scroll(delta) {
        wasm.alacrittyterminal_scroll(this.__wbg_ptr, delta);
    }
    /**
     * Connect to a WebSocket PTY server.
     * @param {string} ws_url
     */
    connect(ws_url) {
        const ptr0 = passStringToWasm0(ws_url, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.alacrittyterminal_connect(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Clean up resources.
     */
    dispose() {
        const ptr = this.__destroy_into_raw();
        wasm.alacrittyterminal_dispose(ptr);
    }
    /**
     * Monotonic counter — incremented once after every successful render.
     * JS can poll this to know when a `feed()` has actually made it to
     * screen, instead of guessing how many RAFs to wait.
     * @returns {number}
     */
    frame_seq() {
        const ret = wasm.alacrittyterminal_frame_seq(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) AlacrittyTerminal.prototype[Symbol.dispose] = AlacrittyTerminal.prototype.free;

const EXPECTED_RESPONSE_TYPES = new Set(['basic', 'cors', 'default']);

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);

            } catch (e) {
                const validResponse = module.ok && EXPECTED_RESPONSE_TYPES.has(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else {
                    throw e;
                }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);

    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };

        } else {
            return instance;
        }
    }
}

function __wbg_get_imports() {
    const imports = {};
    imports.wbg = {};
    imports.wbg.__wbg_Error_e17e777aac105295 = function(arg0, arg1) {
        const ret = Error(getStringFromWasm0(arg0, arg1));
        return ret;
    };
    imports.wbg.__wbg_call_13410aac570ffff7 = function() { return handleError(function (arg0, arg1) {
        const ret = arg0.call(arg1);
        return ret;
    }, arguments) };
    imports.wbg.__wbg_call_a5400b25a865cfd8 = function() { return handleError(function (arg0, arg1, arg2) {
        const ret = arg0.call(arg1, arg2);
        return ret;
    }, arguments) };
    imports.wbg.__wbg_clientHeight_59d075cde7dbe3c7 = function(arg0) {
        const ret = arg0.clientHeight;
        return ret;
    };
    imports.wbg.__wbg_clientWidth_8a498b7a82cae772 = function(arg0) {
        const ret = arg0.clientWidth;
        return ret;
    };
    imports.wbg.__wbg_clipboard_f0744f8afeddc372 = function(arg0) {
        const ret = arg0.clipboard;
        return ret;
    };
    imports.wbg.__wbg_close_6437264570d2d37f = function() { return handleError(function (arg0) {
        arg0.close();
    }, arguments) };
    imports.wbg.__wbg_debug_c906769d2f88c17b = function(arg0) {
        console.debug(arg0);
    };
    imports.wbg.__wbg_devicePixelRatio_de772f7b570607fa = function(arg0) {
        const ret = arg0.devicePixelRatio;
        return ret;
    };
    imports.wbg.__wbg_error_7534b8e9a36f1ab4 = function(arg0, arg1) {
        let deferred0_0;
        let deferred0_1;
        try {
            deferred0_0 = arg0;
            deferred0_1 = arg1;
            console.error(getStringFromWasm0(arg0, arg1));
        } finally {
            wasm.__wbindgen_free(deferred0_0, deferred0_1, 1);
        }
    };
    imports.wbg.__wbg_error_99981e16d476aa5c = function(arg0) {
        console.error(arg0);
    };
    imports.wbg.__wbg_fillRect_a160edfa11fce49b = function(arg0, arg1, arg2, arg3, arg4) {
        arg0.fillRect(arg1, arg2, arg3, arg4);
    };
    imports.wbg.__wbg_fillText_c105710356b625aa = function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
        arg0.fillText(getStringFromWasm0(arg1, arg2), arg3, arg4);
    }, arguments) };
    imports.wbg.__wbg_getContext_15e158d04230a6f6 = function() { return handleError(function (arg0, arg1, arg2) {
        const ret = arg0.getContext(getStringFromWasm0(arg1, arg2));
        return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
    }, arguments) };
    imports.wbg.__wbg_get_458e874b43b18b25 = function() { return handleError(function (arg0, arg1) {
        const ret = Reflect.get(arg0, arg1);
        return ret;
    }, arguments) };
    imports.wbg.__wbg_height_228fe8a75d4d09d6 = function(arg0) {
        const ret = arg0.height;
        return ret;
    };
    imports.wbg.__wbg_info_6cf68c1a86a92f6a = function(arg0) {
        console.info(arg0);
    };
    imports.wbg.__wbg_instanceof_CanvasRenderingContext2d_8c616198ec03b12f = function(arg0) {
        let result;
        try {
            result = arg0 instanceof CanvasRenderingContext2D;
        } catch (_) {
            result = false;
        }
        const ret = result;
        return ret;
    };
    imports.wbg.__wbg_instanceof_Uint8Array_9a8378d955933db7 = function(arg0) {
        let result;
        try {
            result = arg0 instanceof Uint8Array;
        } catch (_) {
            result = false;
        }
        const ret = result;
        return ret;
    };
    imports.wbg.__wbg_instanceof_Window_12d20d558ef92592 = function(arg0) {
        let result;
        try {
            result = arg0 instanceof Window;
        } catch (_) {
            result = false;
        }
        const ret = result;
        return ret;
    };
    imports.wbg.__wbg_length_186546c51cd61acd = function(arg0) {
        const ret = arg0.length;
        return ret;
    };
    imports.wbg.__wbg_length_6bb7e81f9d7713e4 = function(arg0) {
        const ret = arg0.length;
        return ret;
    };
    imports.wbg.__wbg_log_6c7b5f4f00b8ce3f = function(arg0) {
        console.log(arg0);
    };
    imports.wbg.__wbg_measureText_e3f7af8e6d72a37a = function() { return handleError(function (arg0, arg1, arg2) {
        const ret = arg0.measureText(getStringFromWasm0(arg1, arg2));
        return ret;
    }, arguments) };
    imports.wbg.__wbg_navigator_65d5ad763926b868 = function(arg0) {
        const ret = arg0.navigator;
        return ret;
    };
    imports.wbg.__wbg_new_1f3a344cf3123716 = function() {
        const ret = new Array();
        return ret;
    };
    imports.wbg.__wbg_new_8a6f238a6ece86ea = function() {
        const ret = new Error();
        return ret;
    };
    imports.wbg.__wbg_new_e213f63d18b0de01 = function() { return handleError(function (arg0, arg1) {
        const ret = new WebSocket(getStringFromWasm0(arg0, arg1));
        return ret;
    }, arguments) };
    imports.wbg.__wbg_newnoargs_254190557c45b4ec = function(arg0, arg1) {
        const ret = new Function(getStringFromWasm0(arg0, arg1));
        return ret;
    };
    imports.wbg.__wbg_newwithargs_b8065bb443501079 = function(arg0, arg1, arg2, arg3) {
        const ret = new Function(getStringFromWasm0(arg0, arg1), getStringFromWasm0(arg2, arg3));
        return ret;
    };
    imports.wbg.__wbg_now_886b39d7ec380719 = function(arg0) {
        const ret = arg0.now();
        return ret;
    };
    imports.wbg.__wbg_performance_a221af8decc752fb = function(arg0) {
        const ret = arg0.performance;
        return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
    };
    imports.wbg.__wbg_prototypesetcall_3d4a26c1ed734349 = function(arg0, arg1, arg2) {
        Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), arg2);
    };
    imports.wbg.__wbg_queueMicrotask_25d0739ac89e8c88 = function(arg0) {
        queueMicrotask(arg0);
    };
    imports.wbg.__wbg_queueMicrotask_4488407636f5bf24 = function(arg0) {
        const ret = arg0.queueMicrotask;
        return ret;
    };
    imports.wbg.__wbg_readText_c55885e89621dd96 = function(arg0) {
        const ret = arg0.readText();
        return ret;
    };
    imports.wbg.__wbg_readyState_b0d20ca4531d3797 = function(arg0) {
        const ret = arg0.readyState;
        return ret;
    };
    imports.wbg.__wbg_requestAnimationFrame_ddc84a7def436784 = function() { return handleError(function (arg0, arg1) {
        const ret = arg0.requestAnimationFrame(arg1);
        return ret;
    }, arguments) };
    imports.wbg.__wbg_resolve_4055c623acdd6a1b = function(arg0) {
        const ret = Promise.resolve(arg0);
        return ret;
    };
    imports.wbg.__wbg_scale_348633e5fb4d1f00 = function() { return handleError(function (arg0, arg1, arg2) {
        arg0.scale(arg1, arg2);
    }, arguments) };
    imports.wbg.__wbg_send_aa9cb445685f0fd0 = function() { return handleError(function (arg0, arg1, arg2) {
        arg0.send(getArrayU8FromWasm0(arg1, arg2));
    }, arguments) };
    imports.wbg.__wbg_set_453345bcda80b89a = function() { return handleError(function (arg0, arg1, arg2) {
        const ret = Reflect.set(arg0, arg1, arg2);
        return ret;
    }, arguments) };
    imports.wbg.__wbg_setbinaryType_37f3cd35d7775a47 = function(arg0, arg1) {
        arg0.binaryType = __wbindgen_enum_BinaryType[arg1];
    };
    imports.wbg.__wbg_setfillStyle_a9ad5b25cf62a5bc = function(arg0, arg1, arg2) {
        arg0.fillStyle = getStringFromWasm0(arg1, arg2);
    };
    imports.wbg.__wbg_setfont_175a33e591a4080a = function(arg0, arg1, arg2) {
        arg0.font = getStringFromWasm0(arg1, arg2);
    };
    imports.wbg.__wbg_setheight_4fce583024b2d088 = function(arg0, arg1) {
        arg0.height = arg1 >>> 0;
    };
    imports.wbg.__wbg_setlineWidth_069d571345379833 = function(arg0, arg1) {
        arg0.lineWidth = arg1;
    };
    imports.wbg.__wbg_setonclose_159c0332c2d91b09 = function(arg0, arg1) {
        arg0.onclose = arg1;
    };
    imports.wbg.__wbg_setonerror_5d9bff045f909e89 = function(arg0, arg1) {
        arg0.onerror = arg1;
    };
    imports.wbg.__wbg_setonmessage_5e486f326638a9da = function(arg0, arg1) {
        arg0.onmessage = arg1;
    };
    imports.wbg.__wbg_setonopen_3e43af381c2901f8 = function(arg0, arg1) {
        arg0.onopen = arg1;
    };
    imports.wbg.__wbg_setstrokeStyle_3c450999cfcdcd2f = function(arg0, arg1, arg2) {
        arg0.strokeStyle = getStringFromWasm0(arg1, arg2);
    };
    imports.wbg.__wbg_settextBaseline_8af6d434952d07cc = function(arg0, arg1, arg2) {
        arg0.textBaseline = getStringFromWasm0(arg1, arg2);
    };
    imports.wbg.__wbg_setwidth_40a6ed203b92839d = function(arg0, arg1) {
        arg0.width = arg1 >>> 0;
    };
    imports.wbg.__wbg_shift_5fc17c1d52864440 = function(arg0) {
        const ret = arg0.shift();
        return ret;
    };
    imports.wbg.__wbg_stack_0ed75d68575b0f3c = function(arg0, arg1) {
        const ret = arg1.stack;
        const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
        getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
    };
    imports.wbg.__wbg_static_accessor_GLOBAL_8921f820c2ce3f12 = function() {
        const ret = typeof global === 'undefined' ? null : global;
        return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
    };
    imports.wbg.__wbg_static_accessor_GLOBAL_THIS_f0a4409105898184 = function() {
        const ret = typeof globalThis === 'undefined' ? null : globalThis;
        return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
    };
    imports.wbg.__wbg_static_accessor_SELF_995b214ae681ff99 = function() {
        const ret = typeof self === 'undefined' ? null : self;
        return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
    };
    imports.wbg.__wbg_static_accessor_WINDOW_cde3890479c675ea = function() {
        const ret = typeof window === 'undefined' ? null : window;
        return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
    };
    imports.wbg.__wbg_strokeRect_cbddaaf583a64cae = function(arg0, arg1, arg2, arg3, arg4) {
        arg0.strokeRect(arg1, arg2, arg3, arg4);
    };
    imports.wbg.__wbg_then_b33a773d723afa3e = function(arg0, arg1, arg2) {
        const ret = arg0.then(arg1, arg2);
        return ret;
    };
    imports.wbg.__wbg_then_e22500defe16819f = function(arg0, arg1) {
        const ret = arg0.then(arg1);
        return ret;
    };
    imports.wbg.__wbg_warn_e2ada06313f92f09 = function(arg0) {
        console.warn(arg0);
    };
    imports.wbg.__wbg_wbindgencbdrop_eb10308566512b88 = function(arg0) {
        const obj = arg0.original;
        if (obj.cnt-- == 1) {
            obj.a = 0;
            return true;
        }
        const ret = false;
        return ret;
    };
    imports.wbg.__wbg_wbindgendebugstring_99ef257a3ddda34d = function(arg0, arg1) {
        const ret = debugString(arg1);
        const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
        getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
    };
    imports.wbg.__wbg_wbindgenisfunction_8cee7dce3725ae74 = function(arg0) {
        const ret = typeof(arg0) === 'function';
        return ret;
    };
    imports.wbg.__wbg_wbindgenisundefined_c4b71d073b92f3c5 = function(arg0) {
        const ret = arg0 === undefined;
        return ret;
    };
    imports.wbg.__wbg_wbindgenstringget_0f16a6ddddef376f = function(arg0, arg1) {
        const obj = arg1;
        const ret = typeof(obj) === 'string' ? obj : undefined;
        var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len1 = WASM_VECTOR_LEN;
        getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
        getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
    };
    imports.wbg.__wbg_wbindgenthrow_451ec1a8469d7eb6 = function(arg0, arg1) {
        throw new Error(getStringFromWasm0(arg0, arg1));
    };
    imports.wbg.__wbg_width_5671cc92dc930a91 = function(arg0) {
        const ret = arg0.width;
        return ret;
    };
    imports.wbg.__wbg_width_c7a070ac56976582 = function(arg0) {
        const ret = arg0.width;
        return ret;
    };
    imports.wbg.__wbindgen_cast_2241b6af4c4b2941 = function(arg0, arg1) {
        // Cast intrinsic for `Ref(String) -> Externref`.
        const ret = getStringFromWasm0(arg0, arg1);
        return ret;
    };
    imports.wbg.__wbindgen_cast_aaa93aae03c115ab = function(arg0, arg1) {
        // Cast intrinsic for `Closure(Closure { dtor_idx: 1, function: Function { arguments: [], shim_idx: 2, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
        const ret = makeMutClosure(arg0, arg1, 1, __wbg_adapter_6);
        return ret;
    };
    imports.wbg.__wbindgen_cast_b75780d4760f5722 = function(arg0, arg1) {
        // Cast intrinsic for `Closure(Closure { dtor_idx: 128, function: Function { arguments: [Externref], shim_idx: 129, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
        const ret = makeMutClosure(arg0, arg1, 128, __wbg_adapter_9);
        return ret;
    };
    imports.wbg.__wbindgen_init_externref_table = function() {
        const table = wasm.__wbindgen_export_2;
        const offset = table.grow(4);
        table.set(0, undefined);
        table.set(offset + 0, undefined);
        table.set(offset + 1, null);
        table.set(offset + 2, true);
        table.set(offset + 3, false);
        ;
    };

    return imports;
}

function __wbg_init_memory(imports, memory) {

}

function __wbg_finalize_init(instance, module) {
    wasm = instance.exports;
    __wbg_init.__wbindgen_wasm_module = module;
    cachedDataViewMemory0 = null;
    cachedUint8ArrayMemory0 = null;


    wasm.__wbindgen_start();
    return wasm;
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (typeof module !== 'undefined') {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();

    __wbg_init_memory(imports);

    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }

    const instance = new WebAssembly.Instance(module, imports);

    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (typeof module_or_path !== 'undefined') {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (typeof module_or_path === 'undefined') {
        module_or_path = new URL('alacritty_web_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    __wbg_init_memory(imports);

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync };
export default __wbg_init;
