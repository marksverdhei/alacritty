//! WebSocket client for PTY communication.
//!
//! Binary protocol:
//! - 0x00 + bytes = PTY data (bidirectional)
//! - 0x01 + 4x u16 LE (cols, rows, cell_w, cell_h) = resize (client->server)
//! - 0x02 + optional exit code = child exited (server->client)

use wasm_bindgen::prelude::*;
use web_sys::{BinaryType, MessageEvent, WebSocket};

const MSG_PTY_DATA: u8 = 0x00;
const MSG_RESIZE: u8 = 0x01;
const MSG_CHILD_EXIT: u8 = 0x02;

/// WebSocket connection to the PTY server.
pub struct WsConnection {
    ws: WebSocket,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(web_sys::ErrorEvent)>,
    _on_close: Closure<dyn FnMut(web_sys::CloseEvent)>,
}

impl WsConnection {
    /// Create a new WebSocket connection with a callback for incoming PTY data.
    pub fn new<F>(url: &str, on_pty_data: F) -> Result<Self, JsError>
    where
        F: Fn(&[u8]) + 'static,
    {
        let ws = WebSocket::new(url).map_err(|e| JsError::new(&format!("{e:?}")))?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Ok(buf) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
                let arr = js_sys::Uint8Array::new(&buf);
                let data = arr.to_vec();
                if data.is_empty() {
                    return;
                }

                match data[0] {
                    MSG_PTY_DATA => {
                        if data.len() > 1 {
                            on_pty_data(&data[1..]);
                        }
                    },
                    MSG_CHILD_EXIT => {
                        log::info!("Child process exited");
                    },
                    other => {
                        log::warn!("Unknown message type: {other}");
                    },
                }
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        let on_error = Closure::wrap(Box::new(move |e: web_sys::ErrorEvent| {
            log::error!("WebSocket error: {:?}", e.message());
        }) as Box<dyn FnMut(web_sys::ErrorEvent)>);

        let on_close = Closure::wrap(Box::new(move |e: web_sys::CloseEvent| {
            log::info!(
                "WebSocket closed: code={}, reason={}",
                e.code(),
                e.reason()
            );
        }) as Box<dyn FnMut(web_sys::CloseEvent)>);

        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        Ok(Self {
            ws,
            _on_message: on_message,
            _on_error: on_error,
            _on_close: on_close,
        })
    }

    /// Send PTY data to the server.
    pub fn send_pty_data(&self, data: &[u8]) {
        let mut msg = Vec::with_capacity(1 + data.len());
        msg.push(MSG_PTY_DATA);
        msg.extend_from_slice(data);
        let _ = self.ws.send_with_u8_array(&msg);
    }

    /// Send a resize message to the server.
    pub fn send_resize(&self, cols: u16, rows: u16, cell_w: u16, cell_h: u16) {
        let mut msg = Vec::with_capacity(9);
        msg.push(MSG_RESIZE);
        msg.extend_from_slice(&cols.to_le_bytes());
        msg.extend_from_slice(&rows.to_le_bytes());
        msg.extend_from_slice(&cell_w.to_le_bytes());
        msg.extend_from_slice(&cell_h.to_le_bytes());
        let _ = self.ws.send_with_u8_array(&msg);
    }
}

impl Drop for WsConnection {
    fn drop(&mut self) {
        let _ = self.ws.close();
    }
}
