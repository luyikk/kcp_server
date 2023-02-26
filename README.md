# kcpserver
[![Latest Version](https://img.shields.io/crates/v/kcpserver.svg)](https://crates.io/crates/udp_server)
[![Rust Documentation](https://img.shields.io/badge/api-rustdoc-blue.svg)](https://docs.rs/kcpserver)
[![Rust Report Card](https://rust-reportcard.xuri.me/badge/github.com/luyikk/kcp_server)](https://rust-reportcard.xuri.me/report/github.com/luyikk/kcp_server)
[![Rust CI](https://github.com/luyikk/kcp_server/actions/workflows/rust.yml/badge.svg)](https://github.com/luyikk/kcp_server/actions/workflows/rust.yml)
### 最好用的RUST KCP 服务器框架
### Examples Echo
```rust
use kcpserver::prelude::{
    kcp_module::{KcpConfig, KcpNoDelayConfig},
    *,
};
use log::LevelFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .filter_level(LevelFilter::Debug)
        .init();

    let mut config = KcpConfig::default();
    config.nodelay = Some(KcpNoDelayConfig::fastest());
    let kcp_server = KcpListener::new("0.0.0.0:5555", config, 1, |peer| async move {
        log::debug!("create kcp peer:{}", peer.to_string());
        let mut buf = [0; 1024];
        while let Ok(size) = peer.recv(&mut buf).await {
            log::debug!("read peer:{} buff:{}", peer.to_string(), size);
            peer.send(&buf[..size]).await?;
        }
        Ok(())
    })?;
    kcp_server.start().await?;
    Ok(())
}

 ```
