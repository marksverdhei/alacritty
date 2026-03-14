use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use log::{error, info, warn};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use tokio::time::Instant;
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
    // WebSocket upgrade with CORS origin validation.
    let allowed_origins = args.allowed_origins.clone();
    let ws_stream =
        tokio_tungstenite::accept_hdr_async(stream, move |req: &Request, mut resp: Response| {
            // Validate the Origin header.
            let origin = req
                .headers()
                .get(http::header::ORIGIN)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            match (&origin, allowed_origins.is_empty()) {
                // No Origin header and no allowed origins configured: same-origin, allow.
                (None, true) => {},
                // Origin present but no allowed origins configured: reject.
                (Some(o), true) => {
                    warn!("Rejecting connection with Origin '{}' (no allowed origins configured)", o);
                    return Err(http::Response::builder()
                        .status(http::StatusCode::FORBIDDEN)
                        .body(Some("Origin not allowed".to_string()))
                        .unwrap());
                },
                // Allowed origins configured: check if Origin matches.
                (Some(o), false) => {
                    if !allowed_origins.contains(o) {
                        warn!("Rejecting connection with Origin '{}' (not in allowed list)", o);
                        return Err(http::Response::builder()
                            .status(http::StatusCode::FORBIDDEN)
                            .body(Some("Origin not allowed".to_string()))
                            .unwrap());
                    }
                },
                // No Origin header but allowed origins configured: allow (e.g. non-browser client).
                (None, false) => {},
            }

            if let Some(ref o) = origin {
                if let Ok(val) = http::HeaderValue::from_str(o) {
                    resp.headers_mut()
                        .insert(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, val);
                }
            }

            Ok(resp)
        })
        .await?;

    info!("WebSocket connection established with {}", peer);

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // Token authentication if configured -- uses constant-time comparison.
    if let Some(ref expected_token) = args.token {
        info!("Waiting for auth token from {}", peer);
        match ws_stream.next().await {
            Some(Ok(Message::Text(token))) => {
                let provided = token.trim().as_bytes();
                let expected = expected_token.as_bytes();

                // Constant-time comparison to prevent timing attacks.
                let valid =
                    provided.len() == expected.len() && bool::from(provided.ct_eq(expected));

                if !valid {
                    warn!("Auth failed from {}: invalid token", peer);
                    if let Err(e) = ws_sink.send(Message::Close(None)).await {
                        warn!("Failed to send close after auth failure: {}", e);
                    }
                    return Ok(());
                }
                info!("Auth succeeded for {}", peer);
            },
            _ => {
                warn!(
                    "Auth failed from {}: expected text message with token",
                    peer
                );
                if let Err(e) = ws_sink.send(Message::Close(None)).await {
                    warn!("Failed to send close after auth failure: {}", e);
                }
                return Ok(());
            },
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
                },
                Err(e) => {
                    // EIO is expected when the child exits on Linux.
                    if e.kind() != std::io::ErrorKind::Other {
                        error!("PTY read error: {}", e);
                    }
                    break;
                },
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
        if let Err(e) = ws_sink.close().await {
            warn!("Failed to close WebSocket sink: {}", e);
        }
    });

    // Task 3: Read from WebSocket and write to PTY / handle resize.
    // Includes idle timeout: if no input received within the configured period, disconnect.
    let master = pair.master;
    let _ws_tx_client = ws_tx.clone();
    let idle_timeout_secs = args.idle_timeout;
    let ws_read_handle = tokio::task::spawn({
        let writer = Arc::new(std::sync::Mutex::new(writer));
        let master = Arc::new(std::sync::Mutex::new(master));
        async move {
            let idle_timeout = Duration::from_secs(idle_timeout_secs);
            let mut last_input = Instant::now();

            loop {
                let remaining = idle_timeout
                    .checked_sub(last_input.elapsed())
                    .unwrap_or(Duration::ZERO);

                if remaining.is_zero() {
                    warn!(
                        "Idle timeout ({}s) reached for {}",
                        idle_timeout_secs, peer
                    );
                    break;
                }

                tokio::select! {
                    msg = ws_stream.next() => {
                        match msg {
                            Some(Ok(Message::Binary(data))) => {
                                if let Some(client_msg) = protocol::parse_client_message(&data) {
                                    match client_msg {
                                        ClientMessage::Data(payload) => {
                                            last_input = Instant::now();
                                            let writer = Arc::clone(&writer);
                                            if let Err(e) = tokio::task::spawn_blocking(move || {
                                                let mut w = writer.lock().unwrap();
                                                if let Err(e) = w.write_all(&payload) {
                                                    error!("PTY write error: {}", e);
                                                }
                                            })
                                            .await {
                                                error!("PTY write task panicked: {}", e);
                                            }
                                        },
                                        ClientMessage::Resize {
                                            cols,
                                            rows,
                                            cell_w,
                                            cell_h,
                                        } => {
                                            // Resize also counts as activity.
                                            last_input = Instant::now();
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
                                            .await {
                                                error!("PTY resize task panicked: {}", e);
                                            }
                                        },
                                    }
                                }
                            },
                            Some(Ok(Message::Close(_))) => {
                                info!("Client {} sent close", peer);
                                break;
                            },
                            Some(Ok(_)) => {
                                // Ignore text, ping, pong - we only use binary.
                            },
                            Some(Err(e)) => {
                                warn!("WebSocket read error from {}: {}", peer, e);
                                break;
                            },
                            None => break,
                        }
                    }
                    _ = tokio::time::sleep(remaining) => {
                        warn!("Idle timeout ({}s) reached for {}", idle_timeout_secs, peer);
                        break;
                    }
                }
            }
        }
    });

    // Wrap child in Arc<Mutex> so we can kill it on WebSocket close.
    let child = Arc::new(std::sync::Mutex::new(child));

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
                    // portable-pty ExitStatus doesn't expose the raw code easily,
                    // so we use 1 for failure.
                    Some(1u8)
                }
            },
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
            if let Ok(mut child_guard) = child.lock() {
                if let Err(e) = child_guard.kill() {
                    // ESRCH (no such process) is expected if already exited.
                    warn!("Failed to kill child process: {}", e);
                } else {
                    info!("Killed child process for disconnected client {}", peer);
                }
            }
        }
    }

    // Clean up: wait briefly for remaining data to flush.
    if let Err(e) = pty_read_handle.await {
        error!("PTY read task panicked: {}", e);
    }

    // Drop the sender so the write task finishes.
    drop(ws_tx);
    if let Err(e) = ws_write_handle.await {
        error!("WebSocket write task panicked: {}", e);
    }

    info!("Session ended for {}", peer);
    Ok(())
}
