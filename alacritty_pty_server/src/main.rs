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

    /// Allowed WebSocket Origin values (repeatable). If not set, only requests
    /// with no Origin header (same-origin) are accepted.
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

/// Per-IP connection timestamps for rate limiting.
/// Max 5 connections per 10 seconds from the same IP.
struct RateLimiter {
    connections: HashMap<IpAddr, Vec<Instant>>,
}

impl RateLimiter {
    const WINDOW: std::time::Duration = std::time::Duration::from_secs(10);
    const MAX_PER_WINDOW: usize = 5;

    fn new() -> Self {
        Self { connections: HashMap::new() }
    }

    /// Returns true if the connection should be allowed. Also evicts entries
    /// for IPs that have gone quiet — without this, a flood from many one-off
    /// IPs would grow `connections` unboundedly.
    fn check_and_record(&mut self, ip: IpAddr) -> bool {
        self.check_and_record_at(ip, Instant::now())
    }

    /// Testable seam — same as `check_and_record` but with an injected `now`.
    fn check_and_record_at(&mut self, ip: IpAddr, now: Instant) -> bool {
        // Sweep every call: drop any IP whose newest timestamp is outside
        // the window. Cost is O(n) in unique IPs since startup; for a single
        // user this stays in the single digits.
        self.connections.retain(|_, ts| {
            ts.retain(|t| now.duration_since(*t) < Self::WINDOW);
            !ts.is_empty()
        });

        let timestamps = self.connections.entry(ip).or_default();
        if timestamps.len() >= Self::MAX_PER_WINDOW {
            return false;
        }
        timestamps.push(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn allows_up_to_max_in_window() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..RateLimiter::MAX_PER_WINDOW {
            assert!(rl.check_and_record_at(ip("1.2.3.4"), t0));
        }
        // 6th attempt within the window is rejected.
        assert!(!rl.check_and_record_at(ip("1.2.3.4"), t0));
    }

    #[test]
    fn separate_ips_have_separate_budgets() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..RateLimiter::MAX_PER_WINDOW {
            assert!(rl.check_and_record_at(ip("10.0.0.1"), t0));
        }
        // Different IP still has its full budget.
        assert!(rl.check_and_record_at(ip("10.0.0.2"), t0));
    }

    #[test]
    fn window_rolls_off() {
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..RateLimiter::MAX_PER_WINDOW {
            assert!(rl.check_and_record_at(ip("1.2.3.4"), t0));
        }
        assert!(!rl.check_and_record_at(ip("1.2.3.4"), t0));
        // After the window passes, the budget is full again.
        let later = t0 + RateLimiter::WINDOW + std::time::Duration::from_secs(1);
        assert!(rl.check_and_record_at(ip("1.2.3.4"), later));
    }

    #[test]
    fn quiet_ips_are_evicted_to_bound_memory() {
        // Regression test: HashMap<IpAddr, _> used to grow unboundedly because
        // entries for one-off IPs were never removed. Now the sweep evicts them.
        let mut rl = RateLimiter::new();
        let t0 = Instant::now();
        rl.check_and_record_at(ip("1.2.3.4"), t0);
        rl.check_and_record_at(ip("1.2.3.5"), t0);
        rl.check_and_record_at(ip("1.2.3.6"), t0);
        assert_eq!(rl.connections.len(), 3);

        // After the window, a probe from a different IP evicts the stale ones.
        let later = t0 + RateLimiter::WINDOW + std::time::Duration::from_secs(1);
        rl.check_and_record_at(ip("9.9.9.9"), later);
        assert_eq!(rl.connections.len(), 1, "stale entries should be evicted");
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
    if args.allowed_origins.is_empty() {
        info!("CORS: only same-origin requests (no Origin header) accepted");
    } else {
        info!("CORS: allowed origins: {:?}", args.allowed_origins);
    }
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
