use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use log::{error, info, warn};
use tokio::net::TcpListener;

mod protocol;
mod session;

/// WebSocket PTY server for Alacritty WASM port.
#[derive(Parser, Debug, Clone)]
#[command(name = "alacritty-pty-server", version, about)]
pub struct Args {
    /// Address to bind to.
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,

    /// Port to listen on.
    #[arg(long, default_value_t = 7681)]
    port: u16,

    /// Shell command to spawn (defaults to $SHELL or /bin/sh).
    #[arg(long)]
    shell: Option<String>,

    /// Optional shared-secret token for authentication.
    /// When set, the client must send this token as its first WebSocket message.
    #[arg(long)]
    token: Option<String>,

    /// Allowed origins for WebSocket connections (repeatable).
    /// When set, only connections with a matching Origin header are accepted.
    /// When not set, only same-origin (no Origin header) connections are allowed.
    #[arg(long = "allowed-origin")]
    allowed_origins: Vec<String>,
}

impl Args {
    pub fn shell(&self) -> String {
        self.shell
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".to_string())
    }
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Arc::new(Args::parse());
    let addr: SocketAddr = format!("{}:{}", args.bind, args.port)
        .parse()
        .expect("Invalid bind address");

    let listener = TcpListener::bind(&addr)
        .await
        .expect("Failed to bind TCP listener");

    info!("Alacritty PTY server listening on ws://{}", addr);

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                info!("New TCP connection from {}", peer);
                let args = Arc::clone(&args);
                tokio::spawn(async move {
                    if let Err(e) = session::handle_connection(stream, peer, args).await {
                        error!("Session error for {}: {}", peer, e);
                    }
                });
            }
            Err(e) => {
                warn!("Failed to accept connection: {}", e);
            }
        }
    }
}
