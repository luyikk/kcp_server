use std::error::Error;
use udp_server::{UdpServer, Peer};
use std::sync:: Arc;
use std::future::Future;
use futures::executor::block_on;
use std::net::SocketAddr;

/// 为了封装UDP server 去掉无关的泛型参数
/// 定义了一个trait
pub trait UdpListener: Send + Sync {
    fn start(&self) -> Result<(), Box<dyn Error>>;
    fn remove_peer(&self,addr:SocketAddr)->bool;
}

impl<I, R, T, S> UdpListener for UdpServer<I, R, T, S>
    where
        I: Fn(Arc<S>, Arc<Peer<T>>, Vec<u8>) -> R + Send + Sync + 'static,
        R: Future<Output = Result<(), Box<dyn Error>>> + Send,
        T: Send + 'static,
        S: Send + Sync + 'static,
{
    /// 实现 UDPListener的 start
    fn start(&self) -> Result<(), Box<dyn Error>> {
        block_on(async move { self.start().await })
    }

    fn remove_peer(&self,addr:SocketAddr)->bool{
        self.remove_peer(addr)
    }
}




