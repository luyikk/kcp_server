# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`kcpserver` is a Rust library crate providing a KCP reliable transport server framework over UDP. KCP trades higher bandwidth for lower latency compared to TCP — the protocol is implemented here from scratch (no C FFI).

**Crate name**: `kcpserver` (published as `kcpserver` on crates.io)

## Build & Test Commands

```bash
# Build
cargo build

# Run unit tests
cargo test

# Lint (CI gate)
cargo clippy -- -D warnings

# Generate docs
cargo doc --open
```

The project has no binary targets — it's a library consumed via `cargo add kcpserver`.

## Echo Server Testing

The `examples/kcpecho/` directory contains an echo server and two Python test scripts for end-to-end verification. Always test KCP changes against the echo server — never rely on `cargo test` alone (the crate currently has zero `#[test]` functions).

### Quick workflow

```bash
# 1. Build and start the echo server (background)
cargo run --example kcpecho &
sleep 2

# 2. Run the happy-path client (verify echo round-trip)
python3 examples/kcpecho/kcp_client.py

# 3. Run the comprehensive probe suite (all 3 handshake modes + edge cases)
python3 examples/kcpecho/kcp_probe.py

# 4. Stop the server
kill %1
```

### What the scripts cover

| Script | What it tests |
|--------|--------------|
| `kcp_client.py` | Single KCP PUSH (sn=0) → echo verification. Fast happy-path check. |
| `kcp_probe.py` | 5 probes: (1) hot loop with sn=0+sn=1 follow-up, (2) 4-byte conv request mode, (3) key exchange mode, (4) malformed packets (0-byte, 2-byte, bad sn), (5) post-probe health check |

Both scripts:
- Send raw KCP packets over UDP to `127.0.0.1:5555` (no external deps, stdlib only)
- Parse KCP headers to verify responses
- Print `PASS`/`FAIL` verdicts
- Timeout at 2–3 seconds if the server doesn't respond

### Testing specific changes

- **Changed `kcp_listener.rs`** → run `kcp_probe.py` — it exercises all three connection modes, the hot loop, and malformed input
- **Changed `kcp.rs` (KCP protocol)** → run `kcp_client.py` minimally; add targeted probes to `kcp_probe.py` for the changed logic
- **Changed `kcp_config.rs`** → test with a modified echo server config
- **Changed `reader.rs`/`writer.rs`** → `kcp_client.py` covers the read/write path indirectly via echo

## Architecture

The crate has a three-layer design:

### 1. `kcp_module` — Protocol layer (`src/kcp/kcp_module/`)

The raw KCP protocol implementation in `kcp.rs` (~1200 lines). This is the core of the project:

- **`Kcp` struct**: The full KCP control block with send/recv buffers, congestion control (slow start, congestion avoidance), fast retransmit, flow control windows.
- **`KcpSegment`**: Wire-format segment with 24-byte KCP header + payload (conv, cmd, frg, wnd, ts, sn, una, len).
- **`KcpOutput`**: Send abstraction wrapping `UDPPeer` with optional XOR encryption (simple symmetric cipher to evade protocol-based filtering).
- **`KcpConfig`**: User-facing config (MTU, nodelay, window sizes, stream mode, session expiry, flush behavior).
- **`KcpNoDelayConfig`**: Convenience presets; `fastest()` sets nodelay=true, interval=10ms, resend=2, nc=true.
- **Error type** in `error.rs`: Converts between KCP errors and `std::io::Error` (e.g., `RecvQueueEmpty` → `WouldBlock`).

Key protocol details:
- Default MTU: 1400 bytes, MSS = 1376 (MTU - 24-byte header)
- Default send window: 32, recv window: 128
- Stream mode is **enabled by default** (`KcpConfig::default().stream == true`)
- Timestamps are `u32` milliseconds from `SystemTime::now().duration_since(UNIX_EPOCH)`

### 2. `kcp_server` — Server layer (`src/kcp/kcp_server/`)

**`KcpListener`** is the entry point. Created via `KcpListener::new(addr, config, drop_timeout_secs, handler_fn)`:
- Creates a `UdpServer` (from the `udp_server` crate) with per-peer UDP handlers
- The UDP handler implements a **3-mode connection establishment** for each incoming peer:
  1. **Direct mode**: Client sends a full 24+ byte KCP packet with `sn=0` → server creates KCP peer directly
  2. **Conv request mode**: Client sends exactly 4 bytes → server allocates a conv ID and sends it back
  3. **Key exchange mode**: Client sends >4 but <24 bytes → server treats it as an XOR encryption key, generates a conv, and sends both back
- Once a KCP peer is established, the handler fn is spawned as a `tokio::spawn` task
- Runs a background update loop (every 5ms) that calls `Kcp::update()` on peers whose `next_update_time` has elapsed
- Conv IDs start at 1 (monotonic `AtomicU32`)

**`KcpPeer`** (`kcp_peer.rs`) represents a connected client:
- Wraps `Kcp` behind `async_lock::Mutex`
- Uses `AtomicWaker` to wake async tasks waiting on data (when new data arrives via UDP or the pipe breaks)
- `BrokenPipe` detection: when the UDP layer closes, sets flag + wakes waiters so pending `recv()` calls return an error
- Exposes `get_reader()` → `KcpReader` and `get_writer()` → `KcpWriter`

**`KcpReader`** / **`KcpWriter`** (`reader.rs`, `writer.rs`):
- Implement both `tokio::io::AsyncRead`/`AsyncWrite` and `futures::AsyncRead`/`AsyncWrite`
- `KcpReader` has an internal cache: if the next KCP message is larger than the caller's buffer, it reads the full message to cache and serves subsequent reads from cache
- `KcpWriter` wraps `send`/`flush`/`close` as async operations

**`IUdpServer` trait** (`udp_sever.rs`): Abstracts the `UdpServer` for testability (currently unused in tests).

### 3. Public API (`src/lib.rs`)

Everything is re-exported through `kcpserver::prelude`:
- `kcpserver::prelude::kcp_module::{KcpConfig, KcpNoDelayConfig, KcpResult, Error, get_conv, set_conv}`
- `kcpserver::prelude::KcpListener`
- `kcpserver::prelude::KCPPeer` / `KcpPeer` / `KcpReader` / `KcpWriter`

## Key Dependencies

- **`udp_server`**: Provides UDP socket binding and per-peer `UDPPeer`/`UdpReader` abstractions
- **`tokio`** (full features): Async runtime, `AsyncRead`/`AsyncWrite` traits
- **`async-lock`**: `Mutex` used for `Kcp` access (switched from `RwLock` in 1.1.2 — RwLock "doesn't mean much" here)
- **`data-rw`**: Binary read/write helpers for constructing conv response packets
- **`bytes`**: `Bytes`/`BytesMut` for KCP segment buffers
- **`futures`**: `AsyncRead`/`AsyncWrite` impls, `poll_fn`, `FutureExt`
- **`atomic-waker`**: `AtomicWaker` for wake-on-data-arrival pattern

## Code Conventions

- Chinese comments appear alongside English (the author is Chinese-speaking)
- `#[inline]` is used liberally on small methods
- Peer identity is formatted as `(conv-SocketAddr)` via `Display` on `KcpPeer`
- No `unsafe` except for one `set_len` call in `kcp.rs` `input()` method (line 720-722) to avoid zeroing before `read_exact`
- Log levels: `trace` for packet-level events, `debug` for peer lifecycle, `error` for failures

## Publishing

Triggered by pushing a `v*` tag (e.g., `v1.1.5`). The CI workflow in `.github/workflows/publish.yml` creates a GitHub release from `CHANGELOG.md`, then runs `cargo publish`. Update `Cargo.toml` version and `CHANGELOG.md` before tagging.

