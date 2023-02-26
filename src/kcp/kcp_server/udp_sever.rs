use std::error::Error;
use std::future::Future;
use std::io;
use udp_server::prelude::{UDPPeer, UdpReader, UdpServer};

#[async_trait::async_trait]
pub trait IUdpServer<T>: Send + Sync {
    async fn start_udp_server(&self, inner: T) -> io::Result<()>;
}

#[async_trait::async_trait]
impl<I, R, T> IUdpServer<T> for UdpServer<I, T>
where
    I: Fn(UDPPeer, UdpReader, T) -> R + Send + Sync + 'static,
    R: Future<Output = Result<(), Box<dyn Error>>> + Send + 'static,
    T: Sync + Send + Clone + 'static,
{
    async fn start_udp_server(&self, inner: T) -> io::Result<()> {
        self.start(inner).await
    }
}
