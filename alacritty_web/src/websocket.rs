//! WebSocket client for PTY communication.
//!
//! The WebSocket is managed entirely from JavaScript to avoid wasm-bindgen
//! closure reentrancy issues. Data is queued in a JS-side Array and polled
//! from the WASM render loop.

use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::WebSocket;

const MSG_PTY_DATA: u8 = 0x00;
const MSG_RESIZE: u8 = 0x01;

/// WebSocket connection to the PTY server.
///
/// All event handlers are pure JavaScript (via js_sys::Function / eval),
/// so they never call into WASM and can't trigger reentrant closure crashes.
pub struct WsConnection {
    ws: WebSocket,
    /// Incoming data queue, filled by JS onmessage handler, drained by WASM.
    incoming: Rc<js_sys::Array>,
    /// Outgoing messages queued before the connection was open.
    pending: Vec<Vec<u8>>,
    /// Whether we've already flushed pending messages after open.
    flushed: bool,
}

impl WsConnection {
    pub fn new(url: &str) -> Result<Self, JsError> {
        let ws = WebSocket::new(url).map_err(|e| JsError::new(&format!("{e:?}")))?;
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let incoming = Rc::new(js_sys::Array::new());
        // Use JS-side event handlers to avoid wasm-bindgen closure reentrancy.
        // Data is pushed into a JS Array that WASM polls each frame.
        Self::setup_js_handlers(&ws, &incoming)?;

        Ok(Self {
            ws,
            incoming,
            pending: Vec::new(),
            flushed: false,
        })
    }

    fn setup_js_handlers(ws: &WebSocket, incoming: &js_sys::Array) -> Result<(), JsError> {
        // Create JS handler functions that never call into WASM.
        // onmessage: extract PTY data bytes and push to the shared array.
        let onmessage_fn = js_sys::Function::new_with_args(
            "queue",
            "return function(e) {
                if (e.data instanceof ArrayBuffer) {
                    var a = new Uint8Array(e.data);
                    if (a.length > 1 && a[0] === 0) {
                        queue.push(a.slice(1));
                    }
                }
            }",
        );
        let handler = onmessage_fn
            .call1(&JsValue::NULL, incoming)
            .map_err(|e| JsError::new(&format!("Failed to create onmessage: {e:?}")))?;
        ws.set_onmessage(Some(handler.unchecked_ref()));

        // onopen: log. Pending messages are flushed by send_or_queue polling.
        let onopen_fn =
            js_sys::Function::new_no_args("console.log('WebSocket connection opened')");
        ws.set_onopen(Some(&onopen_fn));

        // onerror: log.
        let onerror_fn = js_sys::Function::new_no_args("console.log('WebSocket error')");
        ws.set_onerror(Some(&onerror_fn));

        // onclose: log.
        let onclose_fn = js_sys::Function::new_with_args(
            "e",
            "console.log('WebSocket closed: code=' + e.code)",
        );
        ws.set_onclose(Some(&onclose_fn));

        Ok(())
    }

    /// Drain all incoming PTY data chunks from the JS queue.
    pub fn drain_incoming(&self) -> Vec<Vec<u8>> {
        let len = self.incoming.length();
        if len == 0 {
            return Vec::new();
        }

        let mut chunks = Vec::with_capacity(len as usize);
        for _ in 0..len {
            let val = self.incoming.shift();
            if let Some(arr) = val.dyn_ref::<js_sys::Uint8Array>() {
                chunks.push(arr.to_vec());
            }
        }
        chunks
    }

    /// Check if the WebSocket is open.
    pub fn is_open(&self) -> bool {
        self.ws.ready_state() == WebSocket::OPEN
    }

    /// Flush pending messages if the connection just became open.
    /// Called from the render loop each frame.
    pub fn flush_pending(&mut self) {
        if self.flushed || !self.is_open() {
            return;
        }
        self.flushed = true;
        for msg in self.pending.drain(..) {
            if let Err(e) = self.ws.send_with_u8_array(&msg) {
                log::error!("WebSocket send error while flushing: {e:?}");
            }
        }
    }

    /// Send or queue a raw binary message.
    fn send_or_queue(&mut self, msg: Vec<u8>) {
        if self.is_open() {
            if let Err(e) = self.ws.send_with_u8_array(&msg) {
                log::error!("WebSocket send error: {e:?}");
            }
        } else {
            self.pending.push(msg);
        }
    }

    /// Send PTY data to the server.
    pub fn send_pty_data(&mut self, data: &[u8]) {
        let mut msg = Vec::with_capacity(1 + data.len());
        msg.push(MSG_PTY_DATA);
        msg.extend_from_slice(data);
        self.send_or_queue(msg);
    }

    /// Send a resize message to the server.
    pub fn send_resize(&mut self, cols: u16, rows: u16, cell_w: u16, cell_h: u16) {
        let mut msg = Vec::with_capacity(9);
        msg.push(MSG_RESIZE);
        msg.extend_from_slice(&cols.to_le_bytes());
        msg.extend_from_slice(&rows.to_le_bytes());
        msg.extend_from_slice(&cell_w.to_le_bytes());
        msg.extend_from_slice(&cell_h.to_le_bytes());
        self.send_or_queue(msg);
    }
}

impl Drop for WsConnection {
    fn drop(&mut self) {
        if let Err(e) = self.ws.close() {
            log::error!("WebSocket close error: {e:?}");
        }
    }
}
