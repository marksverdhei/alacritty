use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use log::{error, info, warn};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

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

    /// Maximum number of concurrent sessions (default: 5).
    #[arg(long, default_value_t = 5)]
    max_sessions: usize,

    /// Idle timeout in seconds -- disconnect sessions with no input (default: 300).
    #[arg(long, default_value_t = 300)]
    idle_timeout: u64,
}

impl Args {
    pub fn shell(&self) -> String {
        self.shell
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".to_string())
    }
}

/// Per-IP connection timestamps for rate limiting.
/// Max 5 connections per 10 seconds from the same IP.
struct RateLimiter {
    connections: HashMap<IpAddr, Vec<Instant>>,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

    /// Returns true if the connection should be allowed.
    fn check_and_record(&mut self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let window = std::time::Duration::from_secs(10);

        let timestamps = self.connections.entry(ip).or_default();

        // Remove entries older than the window.
        timestamps.retain(|t| now.duration_since(*t) < window);

        if timestamps.len() >= 5 {
            return false;
        }

        timestamps.push(now);
        true
    }
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Arc::new(Args::parse());

    // Validate shell path exists and is executable.
    let shell_path = args.shell();
    let path = Path::new(&shell_path);
    if !path.exists() {
        error!("Shell '{}' does not exist", shell_path);
        std::process::exit(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            let mode = metadata.permissions().mode();
            if mode & 0o111 == 0 {
                error!("Shell '{}' is not executable", shell_path);
                std::process::exit(1);
            }
        }
    }
    info!("Using shell: {}", shell_path);

    let addr: SocketAddr = format!("{}:{}", args.bind, args.port)
        .parse()
        .expect("Invalid bind address");

    let listener = TcpListener::bind(&addr)
        .await
        .expect("Failed to bind TCP listener");

    info!("Alacritty PTY server listening on ws://{}", addr);
    info!(
        "Max sessions: {}, Idle timeout: {}s",
        args.max_sessions, args.idle_timeout
    );

    let active_sessions = Arc::new(AtomicUsize::new(0));
    let rate_limiter = Arc::new(Mutex::new(RateLimiter::new()));

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                info!("New TCP connection from {}", peer);

                // Rate limiting check.
                {
                    let mut limiter = rate_limiter.lock().await;
                    if !limiter.check_and_record(peer.ip()) {
                        warn!(
                            "Rate limit exceeded for {}, rejecting connection",
                            peer.ip()
                        );
                        drop(stream);
                        continue;
                    }
                }

                // Max sessions check.
                let current = active_sessions.load(Ordering::SeqCst);
                if current >= args.max_sessions {
                    warn!(
                        "Max sessions ({}) reached, rejecting connection from {}",
                        args.max_sessions, peer
                    );
                    drop(stream);
                    continue;
                }

                let args = Arc::clone(&args);
                let active_sessions = Arc::clone(&active_sessions);
                active_sessions.fetch_add(1, Ordering::SeqCst);

                tokio::spawn(async move {
                    if let Err(e) = session::handle_connection(stream, peer, args).await {
                        error!("Session error for {}: {}", peer, e);
                    }
                    active_sessions.fetch_sub(1, Ordering::SeqCst);
                    info!(
                        "Active sessions: {}",
                        active_sessions.load(Ordering::SeqCst)
                    );
                });
            }
            Err(e) => {
                warn!("Failed to accept connection: {}", e);
            }
        }
    }
}
