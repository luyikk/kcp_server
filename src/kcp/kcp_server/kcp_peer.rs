use crate::kcp::kcp_module::prelude::{Kcp, KcpResult};
use aqueue::Actor;
use std::fmt::Formatter;
use std::net::SocketAddr;
use std::sync::Arc;
use udp_server::prelude::IUdpPeer;

pub type KCPPeer = Arc<Actor<KcpPeer>>;

pub struct KcpPeer {
    kcp: Kcp,
    pub conv: u32,
    pub addr: SocketAddr,
    pub next_update_time: u32,
}

impl std::fmt::Display for KcpPeer {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}-{:?})", self.conv, self.addr)
    }
}

impl Drop for KcpPeer {
    #[inline]
    fn drop(&mut self) {
        log::trace!("kcp_peer:{} is Drop", self.conv);
        self.kcp.output.peer.close();
    }
}

impl KcpPeer {
    pub fn new(kcp: Kcp, conv: u32, addr: SocketAddr) -> Arc<Actor<KcpPeer>> {
        Arc::new(Actor::new(Self {
            kcp,
            conv,
            addr,
            next_update_time: Default::default(),
        }))
    }
}

#[async_trait::async_trait]
pub trait IKcpPeer {
    /// 往kcp 压udp数据包
    async fn input(&self, buf: &[u8]) -> KcpResult<usize>;
    /// 查看能读取多少KCP数据包
    fn peek_size(&self) -> KcpResult<usize>;
    /// 从kcp读取数据包
    async fn recv(&self, buf: &mut [u8]) -> KcpResult<usize>;
    /// kcp update
    async fn update(&self, current: u32) -> KcpResult<()>;
    /// 获取addr
    fn get_addr(&self) -> SocketAddr;
    /// 获取conv
    fn get_conv(&self) -> u32;
    /// 获取详细信息
    fn to_string(&self) -> String;
}

#[async_trait::async_trait]
impl IKcpPeer for Actor<KcpPeer> {
    #[inline]
    async fn input(&self, buf: &[u8]) -> KcpResult<usize> {
        self.inner_call(|inner| async move { inner.get_mut().kcp.input(buf) })
            .await
    }
    #[inline]
    fn peek_size(&self) -> KcpResult<usize> {
        unsafe { self.deref_inner().kcp.peek_size() }
    }
    #[inline]
    async fn recv(&self, buf: &mut [u8]) -> KcpResult<usize> {
        self.inner_call(|inner| async move { inner.get_mut().kcp.recv(buf) })
            .await
    }
    #[inline]
    async fn update(&self, current: u32) -> KcpResult<()> {
        self.inner_call(|inner| async move {
            let inner = inner.get_mut();
            if current >= inner.next_update_time {
                inner.kcp.update(current).await
            } else {
                Ok(())
            }
        })
        .await
    }
    #[inline]
    fn get_addr(&self) -> SocketAddr {
        unsafe { self.deref_inner().addr }
    }
    #[inline]
    fn get_conv(&self) -> u32 {
        unsafe { self.deref_inner().conv }
    }
    #[inline]
    fn to_string(&self) -> String {
        unsafe { self.deref_inner().to_string() }
    }
}
