//! Binary protocol message types for WebSocket PTY communication.
//!
//! Wire format:
//! - `0x00` + bytes           = PTY data (bidirectional)
//! - `0x01` + 4x u16 LE      = resize (client -> server): cols, rows, cell_w, cell_h
//! - `0x02` + optional u8     = child exited (server -> client), with optional exit code

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_none() {
        assert!(parse_client_message(&[]).is_none());
    }

    #[test]
    fn unknown_tag_returns_none() {
        assert!(parse_client_message(&[0xff, 1, 2, 3]).is_none());
    }

    #[test]
    fn data_message_extracts_payload() {
        let msg = parse_client_message(&[MSG_DATA, b'h', b'i']);
        match msg {
            Some(ClientMessage::Data(bytes)) => assert_eq!(bytes, b"hi"),
            _ => panic!("expected Data, got {:?}", msg),
        }
    }

    #[test]
    fn data_message_with_only_tag_yields_empty_payload() {
        // A `[MSG_DATA]`-only message (no payload bytes) is valid — represents
        // an "empty input chunk". Used by the server's heartbeat / poke path.
        match parse_client_message(&[MSG_DATA]) {
            Some(ClientMessage::Data(bytes)) => assert!(bytes.is_empty()),
            other => panic!("expected empty Data, got {:?}", other),
        }
    }

    #[test]
    fn resize_message_decodes_little_endian_u16() {
        let mut bytes = vec![MSG_RESIZE];
        bytes.extend_from_slice(&80u16.to_le_bytes());
        bytes.extend_from_slice(&24u16.to_le_bytes());
        bytes.extend_from_slice(&8u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        match parse_client_message(&bytes) {
            Some(ClientMessage::Resize { cols, rows, cell_w, cell_h }) => {
                assert_eq!((cols, rows, cell_w, cell_h), (80, 24, 8, 16));
            },
            other => panic!("expected Resize, got {:?}", other),
        }
    }

    #[test]
    fn resize_message_too_short_returns_none() {
        // Tag + only 6 bytes — needs 8 for the four u16s.
        let bytes = vec![MSG_RESIZE, 0, 0, 0, 0, 0, 0];
        assert!(parse_client_message(&bytes).is_none());
    }

    #[test]
    fn resize_clamps_oversized_cols_rows() {
        // Anti-abuse clamp: cols → 1..=500, rows → 1..=200.
        let mut bytes = vec![MSG_RESIZE];
        bytes.extend_from_slice(&9999u16.to_le_bytes()); // cols
        bytes.extend_from_slice(&9999u16.to_le_bytes()); // rows
        bytes.extend_from_slice(&8u16.to_le_bytes()); // cell_w
        bytes.extend_from_slice(&16u16.to_le_bytes()); // cell_h
        match parse_client_message(&bytes) {
            Some(ClientMessage::Resize { cols, rows, .. }) => {
                assert_eq!(cols, 500, "cols must clamp to 500");
                assert_eq!(rows, 200, "rows must clamp to 200");
            },
            other => panic!("expected Resize, got {:?}", other),
        }
    }

    #[test]
    fn resize_clamps_zero_cols_rows_up() {
        // 0 → 1 (clamp lower bound).
        let mut bytes = vec![MSG_RESIZE];
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&8u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        match parse_client_message(&bytes) {
            Some(ClientMessage::Resize { cols, rows, .. }) => {
                assert_eq!(cols, 1);
                assert_eq!(rows, 1);
            },
            other => panic!("expected Resize, got {:?}", other),
        }
    }

    #[test]
    fn encode_data_prepends_msg_tag() {
        assert_eq!(encode_data(b"abc"), vec![MSG_DATA, b'a', b'b', b'c']);
        assert_eq!(encode_data(&[]), vec![MSG_DATA]);
    }

    #[test]
    fn encode_exit_with_code() {
        assert_eq!(encode_exit(Some(42)), vec![MSG_EXIT, 42]);
    }

    #[test]
    fn encode_exit_without_code() {
        assert_eq!(encode_exit(None), vec![MSG_EXIT]);
    }
}
