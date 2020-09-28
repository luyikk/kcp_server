# kcp_server
性能最牛逼 最好用的RUST KCP 服务器框架

# Examples Echo
```rust
#![feature(async_closure)]
use kcpserver::KcpListener;
use kcpserver::KcpConfig;
use kcpserver::KcpNoDelayConfig;
use std::error::Error;

#[tokio::main]
async fn main()->Result<(),Box<dyn Error>>{
    let mut config = KcpConfig::default();
    config.nodelay = Some(KcpNoDelayConfig::fastest());
    let kcp = KcpListener::<i32, _>::new("0.0.0.0:5555", config).await?;
    kcp.set_buff_input(async move |peer, data| {
         peer.send(&data)?;
         Ok(())
      });
    kcp.start().await?;
    Ok(())
 }
 ```
