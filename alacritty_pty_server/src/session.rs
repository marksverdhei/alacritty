use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::Args;
use crate::protocol::{self, ClientMessage};

/// Handle a single TCP connection: upgrade to WebSocket, optionally authenticate,
/// spawn a PTY, and bridge I/O.
pub async fn handle_connection(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    args: Arc<Args>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // WebSocket upgrade with CORS headers.
    let ws_stream = tokio_tungstenite::accept_hdr_async(stream, |req: &Request, mut resp: Response| {
        // Add CORS header for the demo website.
        resp.headers_mut().insert(
            http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
            http::HeaderValue::from_static("*"),
        );
        let _ = req;
        Ok(resp)
    })
    .await?;

    info!("WebSocket connection established with {}", peer);

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // Token authentication if configured.
    if let Some(ref expected_token) = args.token {
        info!("Waiting for auth token from {}", peer);
        match ws_stream.next().await {
            Some(Ok(Message::Text(token))) => {
                if token.trim() != expected_token.as_str() {
                    warn!("Auth failed from {}: invalid token", peer);
                    let _ = ws_sink
                        .send(Message::Close(None))
                        .await;
                    return Ok(());
                }
                info!("Auth succeeded for {}", peer);
            }
            _ => {
                warn!("Auth failed from {}: expected text message with token", peer);
                let _ = ws_sink
                    .send(Message::Close(None))
                    .await;
                return Ok(());
            }
        }
    }

    // Spawn PTY.
    let pty_system = NativePtySystem::default();
    let pty_size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = pty_system
        .openpty(pty_size)
        .map_err(|e| format!("Failed to open PTY: {}", e))?;

    let shell = args.shell();
    let mut cmd = CommandBuilder::new(&shell);
    cmd.env("TERM", "xterm-256color");

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn shell '{}': {}", shell, e))?;

    // Drop the slave side - the child has it.
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Failed to clone PTY reader: {}", e))?;

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("Failed to take PTY writer: {}", e))?;

    // Channel for sending messages to the WebSocket sink.
    let (ws_tx, mut ws_rx) = mpsc::channel::<Message>(64);

    // Task 1: Read from PTY and forward to WebSocket via channel.
    let ws_tx_pty = ws_tx.clone();
    let pty_read_handle = tokio::task::spawn_blocking(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let msg = protocol::encode_data(&buf[..n]);
                    if ws_tx_pty.blocking_send(Message::Binary(msg.into())).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    // EIO is expected when the child exits on Linux.
                    if e.kind() != std::io::ErrorKind::Other {
                        error!("PTY read error: {}", e);
                    }
                    break;
                }
            }
        }
    });

    // Task 2: Forward channel messages to WebSocket sink.
    let ws_write_handle = tokio::spawn(async move {
        while let Some(msg) = ws_rx.recv().await {
            if ws_sink.send(msg).await.is_err() {
                break;
            }
        }
        let _ = ws_sink.close().await;
    });

    // Task 3: Read from WebSocket and write to PTY / handle resize.
    let master = pair.master;
    let _ws_tx_client = ws_tx.clone();
    let ws_read_handle = tokio::task::spawn({
        let writer = Arc::new(std::sync::Mutex::new(writer));
        let master = Arc::new(std::sync::Mutex::new(master));
        async move {
            while let Some(msg_result) = ws_stream.next().await {
                match msg_result {
                    Ok(Message::Binary(data)) => {
                        if let Some(client_msg) = protocol::parse_client_message(&data) {
                            match client_msg {
                                ClientMessage::Data(payload) => {
                                    let writer = Arc::clone(&writer);
                                    let _ = tokio::task::spawn_blocking(move || {
                                        let mut w = writer.lock().unwrap();
                                        let _ = w.write_all(&payload);
                                    })
                                    .await;
                                }
                                ClientMessage::Resize {
                                    cols,
                                    rows,
                                    cell_w,
                                    cell_h,
                                } => {
                                    let master = Arc::clone(&master);
                                    let _ = tokio::task::spawn_blocking(move || {
                                        let m = master.lock().unwrap();
                                        let size = PtySize {
                                            rows,
                                            cols,
                                            pixel_width: cell_w,
                                            pixel_height: cell_h,
                                        };
                                        if let Err(e) = m.resize(size) {
                                            warn!("PTY resize failed: {}", e);
                                        } else {
                                            info!(
                                                "PTY resized to {}x{} ({}x{} px)",
                                                cols, rows, cell_w, cell_h
                                            );
                                        }
                                    })
                                    .await;
                                }
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        info!("Client {} sent close", peer);
                        break;
                    }
                    Ok(_) => {
                        // Ignore text, ping, pong - we only use binary.
                    }
                    Err(e) => {
                        warn!("WebSocket read error from {}: {}", peer, e);
                        break;
                    }
                }
            }
        }
    });

    // Task 4: Wait for child to exit and send exit notification.
    let ws_tx_exit = ws_tx.clone();
    let child_wait_handle = tokio::task::spawn_blocking(move || {
        let status = child.wait();
        let exit_code = match status {
            Ok(status) => {
                if status.success() {
                    Some(0u8)
                } else {
                    // portable-pty ExitStatus doesn't expose the raw code easily,
                    // so we use 1 for failure.
                    Some(1u8)
                }
            }
            Err(_) => None,
        };
        info!("Child exited with code: {:?}", exit_code);
        let msg = protocol::encode_exit(exit_code);
        let _ = ws_tx_exit.blocking_send(Message::Binary(msg.into()));
    });

    // Wait for the child to exit or the WebSocket read to finish.
    tokio::select! {
        _ = child_wait_handle => {
            info!("Child process exited for {}", peer);
        }
        _ = ws_read_handle => {
            info!("WebSocket closed by client {}", peer);
        }
    }

    // Clean up: wait briefly for remaining data to flush.
    let _ = pty_read_handle.await;

    // Drop the sender so the write task finishes.
    drop(ws_tx);
    let _ = ws_write_handle.await;

    info!("Session ended for {}", peer);
    Ok(())
}
