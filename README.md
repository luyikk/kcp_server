# kcpserver

[![Latest Version](https://img.shields.io/crates/v/kcpserver.svg)](https://crates.io/crates/kcpserver)
[![Rust Documentation](https://img.shields.io/badge/api-rustdoc-blue.svg)](https://docs.rs/kcpserver)
[![Rust Report Card](https://rust-reportcard.xuri.me/badge/github.com/luyikk/kcp_server)](https://rust-reportcard.xuri.me/report/github.com/luyikk/kcp_server)
[![Rust CI](https://github.com/luyikk/kcp_server/actions/workflows/rust.yml/badge.svg)](https://github.com/luyikk/kcp_server/actions/workflows/rust.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)

> **The best Rust KCP server framework** — fast, async, pure-Rust KCP reliable transport over UDP.

**[English](#english) | [中文](#chinese)**

---

## English

### Features

| Feature | Description |
|---------|-------------|
| 🔁 Pure Rust KCP | Full KCP protocol implementation from scratch — no C FFI, no bindings |
| ⚡ Async-first | Built on `tokio`, implements `AsyncRead` / `AsyncWrite` for both `tokio` and `futures` |
| 🔒 UDP-layer encryption | Built-in XOR stream cipher to evade protocol-based DPI filtering |
| 🎯 Stream mode by default | KCP stream mode enabled out of the box — messages are concatenated automatically |
| 🔌 Three handshake modes | Direct KCP, conv request, or encrypted key exchange — server auto-detects |
| ⏱️ Configurable | MTU, nodelay, window sizes, fast retransmit — full KCP tuning surface |

### Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
kcpserver = "1"
tokio = { version = "1", features = ["full"] }
env_logger = "0.11"
anyhow = "1"
```

#### Echo Server

```rust
use kcpserver::prelude::{
    kcp_module::{KcpConfig, KcpNoDelayConfig},
    *,
};
use tokio::io::AsyncReadExt;
use log::LevelFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .filter_level(LevelFilter::Debug)
        .init();

    let mut config = KcpConfig::default();
    config.nodelay = Some(KcpNoDelayConfig::fastest());
    let kcp_server = KcpListener::new("0.0.0.0:5555", config, 10, |peer| async move {
        log::debug!("create kcp peer:{}", peer);
        let mut buf = [0; 1024];
        let mut reader = peer.get_reader();
        let mut writer = peer.get_writer();
        while let Ok(size) = reader.read(&mut buf).await {
            log::debug!("read peer:{} buff:{}", peer, size);
            // AsyncWrite unable to write data of length 0
            writer.write_all(&buf[..size]).await?;
            writer.flush().await?;
            // or
            // peer.write(&buf[..size]).await?;
            // peer.flush().await?;
        }
        // not mandatory
        writer.shutdown().await?;
        log::debug!("kcp peer:{} closed", peer.to_string());
        Ok(())
    })?;
    kcp_server.start().await?;
    Ok(())
}
```

### API Overview

Everything is accessible via `kcpserver::prelude`:

```rust
use kcpserver::prelude::*;
```

#### Core Types

| Type | Description |
|------|-------------|
| `KcpListener` | Entry point — binds UDP socket, manages peers, dispatches handler per connection |
| `KCPPeer` / `KcpPeer` | A connected KCP client (type alias: `KCPPeer = Arc<KcpPeer>`) |
| `KcpReader` | `AsyncRead` wrapper — read KCP messages with tokio or futures |
| `KcpWriter` | `AsyncWrite` wrapper — write KCP messages with tokio or futures |

#### API on `KcpPeer`

```rust
// Reading
let reader: KcpReader = peer.get_reader();
let mut buf = [0u8; 1024];
reader.read(&mut buf).await;            // tokio AsyncRead
// or
peer.recv(&mut buf).await;              // direct recv

// Writing
let writer: KcpWriter = peer.get_writer();
writer.write_all(&data).await;          // tokio AsyncWrite
writer.flush().await;

// Connection info
peer.get_addr();    // -> SocketAddr
peer.get_conv();    // -> u32 (conversation ID)
```

### Configuration

`KcpConfig::default()` provides sensible defaults (stream mode, 1400 MTU). Tune with:

```rust
let mut config = KcpConfig::default();

// Fastest: nodelay, 10ms interval, fast resend=2, no congestion control
config.nodelay = Some(KcpNoDelayConfig::fastest());

// Manual tuning
config.mtu = Some(1200);
config.wnd_size = Some((128, 128));
config.stream = false;      // message mode (default: true = stream mode)
config.flush_write = true;  // flush after every write (default)
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `mtu` | `Option<usize>` | `None` (1400) | Max Transmission Unit |
| `interval` | `Option<u32>` | `None` (100ms) | Internal update interval |
| `nodelay` | `Option<KcpNoDelayConfig>` | `None` | Nodelay preset |
| `wnd_size` | `Option<(u16, u16)>` | `None` (32, 128) | Send/recv window sizes |
| `rx_minrto` | `Option<u32>` | `Some(10)` | Minimal resend timeout (ms) |
| `session_expire` | `Option<Duration>` | `None` (90s) | Session timeout |
| `stream` | `bool` | `true` | Stream mode (true) or message mode (false) |
| `flush_write` | `bool` | `true` | Flush KCP state after each write |
| `flush_acks_input` | `bool` | `true` | Flush ACKs after each input |

#### `KcpNoDelayConfig`

```rust
// Fastest preset
KcpNoDelayConfig::fastest()
// Equivalent to:
KcpNoDelayConfig { nodelay: true, interval: 10, resend: 2, nc: true }

// Default
KcpNoDelayConfig { nodelay: false, interval: 100, resend: 0, nc: false }
```

### Project Structure

```
src/
  lib.rs                       # Public prelude re-exports
  kcp/
    mod.rs
    kcp_module/                 # Protocol layer
      kcp.rs                    # KCP protocol (~1200 lines, core of the crate)
      kcp_config.rs             # KcpConfig, KcpNoDelayConfig
      error.rs                  # Error type + io::Error conversion
      mod.rs
    kcp_server/                 # Server framework layer
      kcp_listener.rs           # KcpListener — entry point, conv management
      kcp_peer.rs               # KcpPeer — per-connection state, read/write
      kcp_peer/
        reader.rs               # KcpReader — AsyncRead impl
        writer.rs               # KcpWriter — AsyncWrite impl
      udp_sever.rs              # IUdpServer trait abstraction
      mod.rs
examples/
  kcpecho/
    main.rs                     # Echo server example
```

### Logging

Set `RUST_LOG` to control verbosity:

```bash
RUST_LOG=debug cargo run --example kcpecho
```

Levels used by the crate:
- `trace` — per-packet KCP events (very verbose)
- `debug` — peer lifecycle, connection state
- `error` — failures

### License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.

---

## 中文

> **最好用的 Rust KCP 服务器框架** — 纯 Rust 实现，异步，高性能。

### 特性

| 特性 | 描述 |
|------|------|
| 🔁 纯 Rust KCP | 从零实现的 KCP 协议，无 C FFI，无外部绑定 |
| ⚡ 原生异步 | 基于 `tokio`，同时实现 `tokio` 和 `futures` 的 `AsyncRead`/`AsyncWrite` |
| 🔒 UDP 层加密 | 内置 XOR 流加密，防止协议被 DPI 识别和阻断 |
| 🎯 默认流模式 | 开箱即用 KCP 流模式，消息自动拼接 |
| 🔌 三种握手 | 直连 KCP / conv 申请 / 密钥交换 — 服务端自动检测 |
| ⏱️ 完全可配 | MTU、nodelay、窗口大小、快速重传 — 完整 KCP 参数调优 |

### 快速开始

添加依赖：

```toml
[dependencies]
kcpserver = "1"
tokio = { version = "1", features = ["full"] }
env_logger = "0.11"
anyhow = "1"
```

#### Echo 服务器

```rust
use kcpserver::prelude::{
    kcp_module::{KcpConfig, KcpNoDelayConfig},
    *,
};
use tokio::io::AsyncReadExt;
use log::LevelFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .filter_level(LevelFilter::Debug)
        .init();

    let mut config = KcpConfig::default();
    config.nodelay = Some(KcpNoDelayConfig::fastest());
    let kcp_server = KcpListener::new("0.0.0.0:5555", config, 10, |peer| async move {
        log::debug!("创建 KCP 连接:{}", peer);
        let mut buf = [0; 1024];
        let mut reader = peer.get_reader();
        let mut writer = peer.get_writer();
        while let Ok(size) = reader.read(&mut buf).await {
            log::debug!("读取 {} 字节:{}", size, peer);
            writer.write_all(&buf[..size]).await?;
            writer.flush().await?;
        }
        writer.shutdown().await?;
        log::debug!("KCP 连接关闭:{}", peer);
        Ok(())
    })?;
    kcp_server.start().await?;
    Ok(())
}
```

### API 总览

所有类型通过 `kcpserver::prelude` 导出：

```rust
use kcpserver::prelude::*;
```

#### 核心类型

| 类型 | 描述 |
|------|------|
| `KcpListener` | 入口点 — 绑定 UDP，管理 peers，为每个连接分发处理函数 |
| `KCPPeer` / `KcpPeer` | KCP 连接对象（类型别名: `KCPPeer = Arc<KcpPeer>`） |
| `KcpReader` | `AsyncRead` 包装 — 用 tokio 或 futures 读取 KCP 消息 |
| `KcpWriter` | `AsyncWrite` 包装 — 用 tokio 或 futures 写入 KCP 消息 |

### 配置

`KcpConfig::default()` 提供合理默认值。按需调优：

```rust
let mut config = KcpConfig::default();

// 最快模式
config.nodelay = Some(KcpNoDelayConfig::fastest());

// 手动调参
config.mtu = Some(1200);
config.wnd_size = Some((128, 128));
config.stream = false;      // 消息模式（默认 true 为流模式）
```

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `mtu` | `Option<usize>` | `None` (1400) | 最大传输单元 |
| `interval` | `Option<u32>` | `None` (100ms) | 内部更新间隔 |
| `nodelay` | `Option<KcpNoDelayConfig>` | `None` | nodelay 预设 |
| `wnd_size` | `Option<(u16, u16)>` | `None` (32, 128) | 发送/接收窗口 |
| `rx_minrto` | `Option<u32>` | `Some(10)` | 最小重传超时 (ms) |
| `session_expire` | `Option<Duration>` | `None` (90s) | 会话过期时间 |
| `stream` | `bool` | `true` | 流模式(true) / 消息模式(false) |
| `flush_write` | `bool` | `true` | 每次写入后刷新 |

#### 连接建立（三种模式）

服务端自动检测客户端使用的握手方式：

1. **直连模式** — 客户端直接发送完整 KCP 包（≥24 字节），`sn=0`
2. **Conv 申请** — 客户端发送 4 字节请求，服务端分配 conv ID 并返回
3. **密钥交换** — 客户端发送 4~23 字节密钥，服务端生成 conv 并开启 XOR 加密

### 日志

```bash
RUST_LOG=debug cargo run --example kcpecho
```

日志级别：
- `trace` — 逐包 KCP 事件
- `debug` — 连接生命周期
- `error` — 异常

### 运行示例

```bash
# 启动 echo 服务器
cargo run --example kcpecho
```

### 协议

MIT 或 Apache-2.0 双协议授权。
