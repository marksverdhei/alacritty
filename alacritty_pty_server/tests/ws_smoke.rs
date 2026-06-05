//! Runtime smoke test for the binary protocol + PTY round-trip.
//!
//! Spawns the built `alacritty-pty-server` on a random localhost port,
//! opens a WebSocket, drives a small shell command through it, and
//! asserts the command's output round-trips back.
//!
//! Exists to catch regressions when bumping `portable-pty` or
//! `tokio-tungstenite` — neither has cargo tests we can rely on, and
//! the only contract that actually matters is "shell input goes in, PTY
//! output comes out".

use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

/// Binary protocol tags — kept in sync with `src/protocol.rs`.
const MSG_DATA: u8 = 0x00;
const MSG_RESIZE: u8 = 0x01;

/// Bind a random free port and immediately release it so the server can claim
/// it. The window between drop and `--port` is small but real; on a quiet CI
/// machine it's been reliable.
fn pick_free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
    let port = l.local_addr().unwrap().port();
    drop(l);
    port
}

/// Block until the server accepts a TCP connection on `port`, or the deadline
/// passes. Returns true if the server came up in time.
fn wait_for_listening(port: u16, deadline: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < deadline {
        if std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(100),
        )
        .is_ok()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    false
}

// Hardcodes /bin/sh as the PTY shell, so only run on Unix-like hosts.
// portable-pty itself supports Windows (ConPTY) but verifying that needs a
// different shell + assertions; out of scope for the dep-bump regression.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn ws_pty_round_trip() {
    let bin = env!("CARGO_BIN_EXE_alacritty-pty-server");
    let port = pick_free_port();

    let child = Command::new(bin)
        .args(["--bind", "127.0.0.1", "--port", &port.to_string(), "--shell", "/bin/sh"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn server");

    // Kill + reap the server even if an assertion below panics.
    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _server = KillOnDrop(child);

    assert!(
        wait_for_listening(port, Duration::from_secs(3)),
        "server didn't start listening within 3s",
    );

    let (mut ws, _resp) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
        .await
        .expect("ws handshake");

    // Resize so the PTY is the size the shell expects.
    let mut resize = vec![MSG_RESIZE];
    resize.extend_from_slice(&80u16.to_le_bytes());
    resize.extend_from_slice(&24u16.to_le_bytes());
    resize.extend_from_slice(&8u16.to_le_bytes());
    resize.extend_from_slice(&16u16.to_le_bytes());
    ws.send(Message::Binary(resize.into())).await.expect("send resize");

    // Send a printf so the *output* differs from the input echo — proves the
    // shell actually executed, not just that the PTY echoed our input.
    let cmd = b"printf 'JOIN-%s\\n' BANG\n";
    let mut payload = vec![MSG_DATA];
    payload.extend_from_slice(cmd);
    ws.send(Message::Binary(payload.into())).await.expect("send data");

    let marker = b"JOIN-BANG";
    let mut acc = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break;
        }
        let recv = tokio::time::timeout(deadline - now, ws.next()).await;
        match recv {
            Err(_) => break,   // overall timeout
            Ok(None) => break, // ws closed
            Ok(Some(Ok(Message::Binary(b)))) => {
                if let Some((&tag, rest)) = b.split_first() {
                    if tag == MSG_DATA {
                        acc.extend_from_slice(rest);
                        if acc.windows(marker.len()).any(|w| w == marker) {
                            break;
                        }
                    }
                }
            },
            Ok(Some(Ok(_))) => {}, // ignore Pings/Text
            Ok(Some(Err(_))) => break,
        }
    }

    let _ = ws.close(None).await;

    let printable: String = String::from_utf8_lossy(&acc).into_owned();
    assert!(
        acc.windows(marker.len()).any(|w| w == marker),
        "expected marker {:?} in PTY output, got {} bytes: {:?}",
        std::str::from_utf8(marker).unwrap(),
        acc.len(),
        printable,
    );
}
