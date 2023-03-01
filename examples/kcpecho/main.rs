use kcpserver::prelude::{
    kcp_module::{KcpConfig, KcpNoDelayConfig},
    *,
};
use log::LevelFilter;
use std::error::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
    let mut writer = peer.get_writer();

    while let Ok(size) = reader.read(&mut buf).await {
        log::debug!("read peer:{} buff:{}", peer, size);
        writer.write_all(&buf[..size]).await?;
        writer.flush().await?;
    }
    writer.shutdown().await?;
    Ok(())
}
