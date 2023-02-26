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
    let kcp_server = KcpListener::new("0.0.0.0:5555", config, 5, |peer| async move {
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
