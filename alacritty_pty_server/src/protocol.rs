/// Binary protocol message types for WebSocket PTY communication.
///
/// Wire format:
/// - `0x00` + bytes           = PTY data (bidirectional)
/// - `0x01` + 4x u16 LE      = resize (client -> server): cols, rows, cell_w, cell_h
/// - `0x02` + optional u8     = child exited (server -> client), with optional exit code

use log::warn;

pub const MSG_DATA: u8 = 0x00;
pub const MSG_RESIZE: u8 = 0x01;
pub const MSG_EXIT: u8 = 0x02;

/// A parsed message from the client.
#[derive(Debug)]
pub enum ClientMessage {
    /// PTY input data.
    Data(Vec<u8>),
    /// Resize request: (cols, rows, cell_width, cell_height).
    Resize {
        cols: u16,
        rows: u16,
        cell_w: u16,
        cell_h: u16,
    },
}

/// Parse a binary WebSocket message from the client.
pub fn parse_client_message(data: &[u8]) -> Option<ClientMessage> {
    if data.is_empty() {
        return None;
    }

    match data[0] {
        MSG_DATA => Some(ClientMessage::Data(data[1..].to_vec())),
        MSG_RESIZE => {
            if data.len() < 9 {
                // Need 1 byte tag + 4 * 2 bytes = 9 bytes total.
                return None;
            }
            let raw_cols = u16::from_le_bytes([data[1], data[2]]);
            let raw_rows = u16::from_le_bytes([data[3], data[4]]);
            let cell_w = u16::from_le_bytes([data[5], data[6]]);
            let cell_h = u16::from_le_bytes([data[7], data[8]]);

            // Clamp cols to 1..=500 and rows to 1..=200 to prevent abuse.
            let cols = raw_cols.clamp(1, 500);
            let rows = raw_rows.clamp(1, 200);
            if cols != raw_cols || rows != raw_rows {
                warn!(
                    "Resize values clamped: cols {}→{}, rows {}→{}",
                    raw_cols, cols, raw_rows, rows
                );
            }

            Some(ClientMessage::Resize {
                cols,
                rows,
                cell_w,
                cell_h,
            })
        }
        _ => None,
    }
}

/// Build a PTY data message to send to the client.
pub fn encode_data(payload: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(1 + payload.len());
    msg.push(MSG_DATA);
    msg.extend_from_slice(payload);
    msg
}

/// Build a child-exited message to send to the client.
pub fn encode_exit(exit_code: Option<u8>) -> Vec<u8> {
    match exit_code {
        Some(code) => vec![MSG_EXIT, code],
        None => vec![MSG_EXIT],
    }
}
