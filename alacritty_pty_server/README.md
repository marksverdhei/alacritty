# alacritty_pty_server

WebSocket PTY server that bridges a browser-side terminal (e.g. the WASM port in
`alacritty_web`) to a real shell process running on the server. Speaks a small
binary protocol over a single WebSocket connection per session.

## Build

```sh
cargo build --release -p alacritty_pty_server
```

The binary lands at `target/release/alacritty-pty-server`. No runtime dependencies
beyond a libc with PTY support (`portable-pty 0.9` brings `serial2` instead of the
old `termios` crate, so the binary also cross-compiles for Android targets — see
`feedback_portable_pty_android.md`).

## Run

```sh
alacritty-pty-server                                # /bin/sh, no auth, localhost:7681
alacritty-pty-server --shell /bin/bash              # explicit shell
alacritty-pty-server --bind 0.0.0.0 --port 8080     # listen everywhere
alacritty-pty-server --token "$(openssl rand -hex 16)"   # require auth token
alacritty-pty-server --allowed-origin https://example.com   # CORS allowlist
```

### CLI flags

| Flag | Default | Purpose |
|---|---|---|
| `--bind` | `127.0.0.1` | Interface to listen on. Use `0.0.0.0` to expose. |
| `--port` | `7681` | TCP port. |
| `--shell` | `$SHELL` or `/bin/sh` | Shell binary to spawn for each session. |
| `--token` | _none_ | Shared secret; if set, the client must send it as the first WS text message before any PTY traffic. |
| `--max-sessions` | `5` | Concurrent session ceiling. Excess connections rejected. |
| `--idle-timeout` | `300` (seconds) | Disconnect sessions with no client input for this long. |
| `--allowed-origin` | _none_ (repeatable) | Allowed `Origin` header values. When unset, only requests with no `Origin` (i.e. same-origin) are accepted. |
| `--rate-limit-max` | `5` | Max new connections per IP per 10 s window. `0` disables rate limiting — used by the Playwright suite, which opens a fresh WebSocket per test. |

## Protocol

After optional token authentication, each terminal protocol WebSocket message is
binary. The first byte is a tag:

| Tag | Direction | Body | Meaning |
|---|---|---|---|
| `0x00` (DATA) | both | raw bytes | PTY input (client→server) or PTY output (server→client). |
| `0x01` (RESIZE) | client→server | 4× u16 LE | `cols, rows, cell_w, cell_h`. cols clamped 1..=500, rows clamped 1..=200. |
| `0x02` (EXIT) | server→client | optional u8 | Child process exited. The byte, if present, is the exit code (0..=255). |

When `--token` is set, the **first** WS message must be a text frame containing
the token string. Subsequent binary `DATA` frames carry PTY input as normal.
Server uses constant-time comparison (`subtle` crate). The WASM client exposes
this as `connect_with_token(wsUrl, token)`.

## Security considerations

- **Origin allowlist (`--allowed-origin`)** is checked during the HTTP upgrade
  handshake, before the WS protocol even starts. Set this in any deployment
  reachable over the network.
- **Rate limit (`--rate-limit-max`)** is per-IP per-10-s. The default 5/10s
  suits human use; raise (or set `0`) for test environments.
- **Token auth (`--token`)** is the only way to prevent random clients on
  allowed origins from spawning a shell. The token is compared in constant time.
- **Resize clamps** in `protocol.rs` cap cols at 500 and rows at 200 — prevents
  a malicious or buggy client from requesting absurd PTY sizes that would let
  the shell spawn a huge buffer.
- **Idle timeout** kills abandoned sessions so a half-open WebSocket can't sit
  forever holding a shell.
- The server runs whatever `--shell` points to; if you let untrusted users
  reach it, they get full shell access. There is no sandboxing.

## Embedding in a Playwright test

The repo's `demo/playwright.config.ts` boots this server as a second `webServer`
entry alongside Vite. The catch is that Playwright's default HTTP probe fails
because the server only speaks WebSocket — use the TCP-level `port` check
instead of `url`:

```ts
{
  command: '../target/release/alacritty-pty-server --rate-limit-max 0 --allowed-origin http://localhost:5173',
  port: 7681,
  reuseExistingServer: true,
}
```

The `demo/package.json` `pretest` hook (`cargo build --release -p alacritty_pty_server`)
ensures the binary is fresh before Playwright tries to spawn it.

## Crate layout

| File | Role |
|---|---|
| `src/main.rs` | CLI parsing, TCP listen loop, per-connection dispatch, rate limiter. |
| `src/session.rs` | One session: WebSocket upgrade with origin check, optional token auth, PTY spawn, bidirectional pump, idle-timeout enforcement, child-exit notification. |
| `src/protocol.rs` | Binary tag constants + `parse_client_message`. |
