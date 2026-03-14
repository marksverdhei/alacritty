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

/// Check whether the Origin header is allowed based on the configured allowed origins.
/// If no allowed origins are configured, only requests without an Origin header are accepted
/// (same-origin policy).
fn is_origin_allowed(origin: Option<&http::HeaderValue>, allowed_origins: &[String]) -> bool {
    match origin {
        None => {
            // No Origin header means same-origin; always allowed.
            true
        }
        Some(origin_value) => {
            if allowed_origins.is_empty() {
                // No allowed origins configured: reject cross-origin requests.
                false
            } else {
                let origin_str = origin_value.to_str().unwrap_or("");
                allowed_origins.iter().any(|allowed| allowed == origin_str)
            }
        }
    }
}

/// Handle a single TCP connection: upgrade to WebSocket, optionally authenticate,
/// spawn a PTY, and bridge I/O.
pub async fn handle_connection(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    args: Arc<Args>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let allowed_origins = args.allowed_origins.clone();

    // WebSocket upgrade with origin validation.
    let ws_stream =
        tokio_tungstenite::accept_hdr_async(stream, |req: &Request, mut resp: Response| {
            let origin = req.headers().get(http::header::ORIGIN);

            if !is_origin_allowed(origin, &allowed_origins) {
                let origin_str = origin
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("<unknown>");
                warn!(
                    "Rejected connection from {}: origin '{}' not allowed",
                    peer, origin_str
                );
                *resp.status_mut() = http::StatusCode::FORBIDDEN;
                return Err(http::Response::builder()
                    .status(http::StatusCode::FORBIDDEN)
                    .body(Some("Origin not allowed".to_string()))
                    .unwrap());
            }

            // If origin is allowed and present, echo it back in CORS header.
            if let Some(origin_value) = origin {
                resp.headers_mut().insert(
                    http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                    origin_value.clone(),
                );
            }

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
                    if let Err(e) = ws_sink.send(Message::Close(None)).await {
                        warn!(
                            "Failed to send close after auth failure to {}: {}",
                            peer, e
                        );
                    }
                    return Ok(());
                }
                info!("Auth succeeded for {}", peer);
            }
            _ => {
                warn!(
                    "Auth failed from {}: expected text message with token",
                    peer
                );
                if let Err(e) = ws_sink.send(Message::Close(None)).await {
                    warn!(
                        "Failed to send close after auth failure to {}: {}",
                        peer, e
                    );
                }
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

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn shell '{}': {}", shell, e))?;

    // Wrap child in Arc<Mutex> so we can kill it on WebSocket close.
    let child = Arc::new(std::sync::Mutex::new(child));

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
                    if ws_tx_pty
                        .blocking_send(Message::Binary(msg.into()))
                        .is_err()
                    {
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
            if let Err(e) = ws_sink.send(msg).await {
                warn!("WebSocket send error: {}", e);
                break;
            }
        }
        if let Err(e) = ws_sink.close().await {
            warn!("WebSocket close error: {}", e);
        }
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
                                    if let Err(e) = tokio::task::spawn_blocking(move || {
                                        let mut w = writer.lock().unwrap();
                                        if let Err(e) = w.write_all(&payload) {
                                            error!("PTY write error: {}", e);
                                        }
                                    })
                                    .await
                                    {
                                        error!("PTY write task failed: {}", e);
                                    }
                                }
                                ClientMessage::Resize {
                                    cols,
                                    rows,
                                    cell_w,
                                    cell_h,
                                } => {
                                    let master = Arc::clone(&master);
                                    if let Err(e) = tokio::task::spawn_blocking(move || {
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
                                    .await
                                    {
                                        error!("PTY resize task failed: {}", e);
                                    }
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
    let child_for_wait = Arc::clone(&child);
    let child_wait_handle = tokio::task::spawn_blocking(move || {
        let status = child_for_wait.lock().unwrap().wait();
        let exit_code = match status {
            Ok(status) => {
                if status.success() {
                    Some(0u8)
                } else {
                    Some(1u8)
                }
            }
            Err(_) => None,
        };
        info!("Child exited with code: {:?}", exit_code);
        let msg = protocol::encode_exit(exit_code);
        if let Err(e) = ws_tx_exit.blocking_send(Message::Binary(msg.into())) {
            warn!("Failed to send exit notification: {}", e);
        }
    });

    // Wait for the child to exit or the WebSocket read to finish.
    tokio::select! {
        _ = child_wait_handle => {
            info!("Child process exited for {}", peer);
        }
        _ = ws_read_handle => {
            info!("WebSocket closed by client {}", peer);
            // Kill the child process if still running.
            let child_for_kill = Arc::clone(&child);
            if let Err(e) = tokio::task::spawn_blocking(move || {
                let mut child = child_for_kill.lock().unwrap();
                info!("Killing child process after WebSocket close");
                if let Err(e) = child.kill() {
                    warn!("Failed to kill child process: {}", e);
                }
            }).await {
                error!("Child kill task failed: {}", e);
            }
        }
    }

    // Clean up: wait briefly for remaining data to flush.
    if let Err(e) = pty_read_handle.await {
        warn!("PTY read task join error: {}", e);
    }

    // Drop the sender so the write task finishes.
    drop(ws_tx);
    if let Err(e) = ws_write_handle.await {
        warn!("WebSocket write task join error: {}", e);
    }

    info!("Session ended for {}", peer);
    Ok(())
}
