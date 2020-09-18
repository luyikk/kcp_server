use tokio::sync::Mutex;
use udp_server:: TokenStore;
use crate::kcp_module::{Kcp, KcpResult};
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::net::SocketAddr;
use log::*;
/// KCP LOCK
/// 将KCP 对象操作完全上锁,以保证内存安全 通知简化调用
pub struct KcpLock(Arc<Mutex<Kcp>>);
unsafe impl Send for KcpLock{}
unsafe impl Sync for KcpLock{}

impl KcpLock{
    pub async fn peeksize(&self)-> KcpResult<usize>{
        self.0.lock().await.peeksize()
    }

    pub async fn input(&self, buf: &[u8]) -> KcpResult<usize>{
        self.0.lock().await.input(buf)
    }

    pub async fn recv(&self, buf: &mut [u8]) -> KcpResult<usize>{
        self.0.lock().await.recv(buf)
    }

    pub async fn send(&self, buf: &[u8]) -> KcpResult<usize>{
        self.0.lock().await.send(buf)
    }

    pub async fn update(&self, current: u32) ->  KcpResult<()>{
        self.0.lock().await.update(current).await
    }

    pub async fn flush(&self) -> KcpResult<()>{
        self.0.lock().await.flush().await
    }

    pub async fn flush_async(&self)->  KcpResult<()>{
        self.0.lock().await.flush_async().await
    }
}


impl Kcp{
    pub fn get_lock(self)->KcpLock{
        let recv = Arc::new(Mutex::new(self));
        KcpLock(recv)
    }
}



/// KCP Peer
/// UDP的包进入 KCP PEER 经过KCP 处理后 输出
/// 输入的包 进入KCP PEER处理,然后 输出到UDP PEER SEND TO
/// 同时还需要一个UPDATE 线程 去10MS 一次的运行KCP UPDATE
/// token 用于扩赞逻辑上下文
pub struct KcpPeer<T: Send> {
    pub kcp:KcpLock,
    pub conv: u32,
    pub addr: SocketAddr,
    pub token: Mutex<TokenStore<T>>,
    pub last_rev_time: AtomicI64
}

impl<T:Send> Drop for KcpPeer<T>{
    fn drop(&mut self) {
        info!("kcp_peer:{} is Drop",self.conv);
    }
}

unsafe impl<T: Send> Send for KcpPeer<T>{}
unsafe impl<T: Send> Sync for KcpPeer<T>{}

/// 简化KCP PEER 函数
impl <T:Send> KcpPeer<T>{
    pub async fn peeksize(&self)-> KcpResult<usize>{
        self.kcp.peeksize().await
    }

    pub async fn input(&self, buf: &[u8]) -> KcpResult<usize>{
        self.kcp.input(buf).await
    }

    pub async fn recv(&self, buf: &mut [u8]) -> KcpResult<usize>{
        self.kcp.recv(buf).await
    }

    pub async fn send(&self, buf: &[u8]) -> KcpResult<usize>{
        self.kcp.send(buf).await
    }

    pub async fn update(&self, current: u32) ->  KcpResult<()>{
        self.kcp.update(current).await
    }

    pub async fn flush(&self) -> KcpResult<()>{
        self.kcp.flush().await
    }

    pub async fn flush_async(&self)->  KcpResult<()>{
        self.kcp.flush_async().await
    }

}