use kcpserver::prelude::{
    kcp_module::{KcpConfig, KcpNoDelayConfig},
    *,
};
use log::LevelFilter;
use std::error::Error;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .filter_level(LevelFilter::Debug)
        .init();
    let mut config = KcpConfig::default();
    config.nodelay = Some(KcpNoDelayConfig::fastest());
    KcpListener::new("0.0.0.0:5555", config, 5, input)?
        .start()
        .await?;
    Ok(())
}

#[inline]
async fn input(peer: KCPPeer) -> Result<(), Box<dyn Error>> {
    log::debug!("create kcp peer:{}", peer.to_string());
    let mut buf = [0; 1024];

    //let mut reader = KcpReader::from(&peer);
    let mut reader = peer.get_reader();

    while let Ok(size) = reader.read(&mut buf).await {
        log::debug!("read peer:{} buff:{}", peer.to_string(), size);
        peer.send(&buf[..size]).await?;
    }

    Ok(())
}
