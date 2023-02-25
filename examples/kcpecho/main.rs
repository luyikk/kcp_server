#![feature(async_closure)]
use kcpserver::KcpConfig;
use kcpserver::KcpListener;
use kcpserver::KcpNoDelayConfig;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // let mut config = KcpConfig::default();
    // config.nodelay = Some(KcpNoDelayConfig::fastest());
    // let kcp = KcpListener::<(), _>::new("0.0.0.0:5555", config,30).await?;
    // kcp.set_buff_input(async move |peer, data| {
    //     peer.send(&data).await?;
    //     Ok(())
    // });
    // kcp.start().await?;
    Ok(())
}
