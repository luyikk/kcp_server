use std::io;
use udp_server::prelude::UdpServer;

#[async_trait::async_trait]
pub trait IUdpServer<T>: Send + Sync {
    async fn start(&self, inner: T) -> io::Result<()>;
}

#[async_trait::async_trait]
impl<I: Send + Sync, T: Send + Sync> IUdpServer<T> for UdpServer<I, T> {
    async fn start(&self, inner: T) -> io::Result<()> {
        self.start(inner).await
    }
}
