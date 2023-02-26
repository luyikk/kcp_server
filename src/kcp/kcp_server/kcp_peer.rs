use crate::kcp::kcp_module::prelude::{Kcp, KcpResult};
use aqueue::Actor;
use futures_lite::future::poll_fn;
use std::fmt::Formatter;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use udp_server::prelude::IUdpPeer;

pub type KCPPeer = Arc<Actor<KcpPeer>>;

/// kcp peer 用于管理玩家上下文 以及 读取 和发送 数据
pub struct KcpPeer {
    kcp: Kcp,
    wake: atomic_waker::AtomicWaker,
    is_broken_pipe: AtomicBool,
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
    }
}

impl KcpPeer {
    pub fn new(kcp: Kcp, conv: u32, addr: SocketAddr) -> Arc<Actor<KcpPeer>> {
        Arc::new(Actor::new(Self {
            kcp,
            wake: Default::default(),
            is_broken_pipe: Default::default(),
            conv,
            addr,
            next_update_time: Default::default(),
        }))
    }

    #[inline]
    fn peek_size(&self) -> KcpResult<usize> {
        match self.kcp.peek_size() {
            Ok(size) => Ok(size),
            Err(err) => {
                if self.is_broken_pipe.load(Ordering::Acquire) {
                    //如果udp层关闭那么 返回 BrokenPipe
                    Err(crate::prelude::kcp_module::Error::BrokenPipe)
                } else {
                    Err(err)
                }
            }
        }
    }

    #[inline]
    fn input(&mut self, buf: &[u8]) -> KcpResult<usize> {
        match self.kcp.input(buf) {
            Ok(usize) => {
                self.wake.wake();
                Ok(usize)
            }
            Err(err) => Err(err),
        }
    }
}

#[async_trait::async_trait]
pub(crate) trait KcpIO {
    /// 往kcp 压udp数据包
    async fn input(&self, buf: &[u8]) -> KcpResult<usize>;
    /// udp 层关闭时设置,设置完将导致kcp无法读取
    fn set_broken_pipe(&self);
    /// 查看能读取多少KCP数据包
    fn peek_size(&self) -> KcpResult<usize>;
    /// 关闭kcp peer
    fn close(&self);
    /// 从kcp读取数据包
    async fn recv_buf(&self, buf: &mut [u8]) -> KcpResult<usize>;
    /// kcp update
    async fn update(&self, current: u32) -> KcpResult<()>;
}

#[async_trait::async_trait]
impl KcpIO for Actor<KcpPeer> {
    #[inline]
    async fn input(&self, buf: &[u8]) -> KcpResult<usize> {
        self.inner_call(|inner| async move { inner.get_mut().input(buf) })
            .await
    }

    #[inline]
    fn set_broken_pipe(&self) {
        unsafe {
            self.deref_inner()
                .is_broken_pipe
                .store(true, Ordering::Release);
        }
    }

    #[inline]
    fn peek_size(&self) -> KcpResult<usize> {
        unsafe { self.deref_inner().peek_size() }
    }

    #[inline]
    fn close(&self) {
        unsafe {
            if !self.deref_inner().is_broken_pipe.load(Acquire) {
                self.deref_inner().kcp.output.peer.close();
            }
        }
    }

    #[inline]
    async fn recv_buf(&self, buf: &mut [u8]) -> KcpResult<usize> {
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
}

#[async_trait::async_trait]
pub trait IKcpPeer {
    /// 获取addr
    fn get_addr(&self) -> SocketAddr;
    /// 获取conv
    fn get_conv(&self) -> u32;
    /// 读取数据包
    async fn recv(&self, buf: &mut [u8]) -> KcpResult<usize>;
    /// 获取详细信息
    fn to_string(&self) -> String;
}

#[async_trait::async_trait]
impl IKcpPeer for Actor<KcpPeer> {
    #[inline]
    fn get_addr(&self) -> SocketAddr {
        unsafe { self.deref_inner().addr }
    }
    #[inline]
    fn get_conv(&self) -> u32 {
        unsafe { self.deref_inner().conv }
    }

    #[inline]
    async fn recv(&self, buf: &mut [u8]) -> KcpResult<usize> {
        use futures_lite::future::FutureExt;
        struct WaitInput<'a> {
            peer: &'a Actor<KcpPeer>,
        }

        impl<'a> Future for WaitInput<'a> {
            type Output = KcpResult<usize>;

            #[inline]
            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                macro_rules! try_recv {
                    () => {
                        match self.peer.peek_size() {
                            Ok(size) => {
                                if size > 0 {
                                    return Poll::Ready(Ok(size));
                                }
                            }
                            Err(crate::prelude::kcp_module::Error::BrokenPipe) => {
                                return Poll::Ready(Err(
                                    crate::prelude::kcp_module::Error::BrokenPipe,
                                ))
                            }
                            Err(_) => {}
                        }
                    };
                }
                try_recv!();
                unsafe {
                    self.peer.deref_inner().wake.register(cx.waker());
                }
                try_recv!();
                Poll::Pending
            }
        }

        let mut wait = WaitInput { peer: self };
        let size = poll_fn(|cx| wait.poll(cx)).await?;
        if buf.len() < size {
            Err(crate::prelude::kcp_module::Error::UserBufTooSmall(size))
        } else {
            self.recv_buf(buf).await
        }
    }

    #[inline]
    fn to_string(&self) -> String {
        unsafe { self.deref_inner().to_string() }
    }
}
