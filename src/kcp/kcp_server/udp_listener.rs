use std::error::Error;
use crate::udp::{UdpServer, RecvType};
use crate::udp::SendUDP;
use std::sync:: Arc;
use std::future::Future;
use std::net::SocketAddr;
use async_trait::*;
use tokio::sync::mpsc::UnboundedSender;


/// 为了封装UDP server 去掉无关的泛型参数
/// 定义了一个trait
#[async_trait]
pub trait UdpListener: Send + Sync {
    fn get_msg_tx(&self)->Option<UnboundedSender<RecvType>>;
    async fn start(&self) -> Result<(), Box<dyn Error>>;
}

#[async_trait]
impl<I, R, S> UdpListener for UdpServer<I, R, S>
    where
        I: Fn(Arc<S>, SendUDP, SocketAddr, Vec<u8>) -> R + Send + Sync + 'static,
        R: Future<Output = Result<(), Box<dyn Error>>> + Send,
        S: Send + Sync + 'static,
{
    fn get_msg_tx(&self) -> Option<UnboundedSender<RecvType>> {
       self.get_msg_tx()
    }

    /// 实现 UDPListener的 start
    async fn start(&self) -> Result<(), Box<dyn Error>> {
        self.start().await?;
        Ok(())
    }
}




