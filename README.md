# kcp_server
fast kcp server frame

# Examples Echo
```rust
#![feature(async_closure)]
use kcp_server::KcpListener;
use kcp_server::KcpConfig;
use kcp_server::KcpNoDelayConfig;
use std::error::Error;

#[tokio::main]
async fn main()->Result<(),Box<dyn Error>>{
    let mut config = KcpConfig::default();
    config.nodelay = Some(KcpNoDelayConfig::fastest());
    let kcp = KcpListener::<i32, _>::new("0.0.0.0:5555", config).await?;
    kcp.set_buff_input(async move |peer, data| {
         peer.send(&data).await?;
         Ok(())
      }).await;
    kcp.start().await?;
    Ok(())
 }
 ```
