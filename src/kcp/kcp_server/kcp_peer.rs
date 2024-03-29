mod reader;
mod writer;

use crate::kcp::kcp_module::prelude::{Kcp, KcpResult};
use async_lock::MutexGuard;
use futures::{
    future::{poll_fn, BoxFuture},
    FutureExt,
};
pub use reader::*;
use std::fmt::Formatter;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::task::{ready, Context, Poll};
pub use writer::*;

pub type KCPPeer = Arc<KcpPeer>;

/// kcp peer 用于管理玩家上下文 以及 读取 和发送 数据
pub struct KcpPeer {
    kcp: async_lock::Mutex<Kcp>,
    wake: atomic_waker::AtomicWaker,
    is_broken_pipe: AtomicBool,
    pub conv: u32,
    pub addr: SocketAddr,
    pub next_update_time: AtomicU32,
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
        log::trace!("kcp_peer:{} is Drop", self);
    }
}

impl KcpPeer {
    pub fn new(kcp: Kcp, conv: u32, addr: SocketAddr) -> Arc<KcpPeer> {
        Arc::new(Self {
            kcp: async_lock::Mutex::new(kcp),
            wake: Default::default(),
            is_broken_pipe: Default::default(),
            conv,
            addr,
            next_update_time: Default::default(),
        })
    }

    /// 往kcp 压udp数据包
    #[inline]
    pub(crate) async fn input(&self, buf: &mut [u8], decode: bool) -> KcpResult<usize> {
        match self.kcp.lock().await.input(buf, decode) {
            Ok(usize) => {
                self.wake.wake();
                self.next_update_time.store(0, Ordering::Release);
                Ok(usize)
            }
            Err(err) => Err(err),
        }
    }

    /// udp 层关闭时设置,设置完将导致kcp无法读取
    #[inline]
    pub(crate) fn set_broken_pipe(&self) {
        self.is_broken_pipe.store(true, Ordering::Release);
        self.wake.wake();
    }

    /// 关闭kcp peer
    #[inline]
    pub(crate) async fn close(&self) {
        if !self.is_broken_pipe.load(Ordering::Acquire) {
            self.kcp.lock().await.output.peer.close();
        }
    }

    /// 从kcp读取数据包
    #[inline]
    pub(crate) async fn recv_buf(&self, buf: &mut [u8]) -> KcpResult<usize> {
        self.kcp.lock().await.recv(buf)
    }

    /// 是否需要update
    #[inline]
    pub(crate) fn need_update(&self, current: u32) -> bool {
        self.next_update_time.load(Ordering::Acquire) < current
    }

    /// kcp update
    #[inline]
    pub(crate) async fn update(&self, current: u32) -> KcpResult<()> {
        if current >= self.next_update_time.load(Ordering::Acquire) {
            let mut kcp = self.kcp.lock().await;
            kcp.update(current).await?;
            self.next_update_time
                .store(kcp.check(current) + current, Ordering::Release);
            Ok(())
        } else {
            Ok(())
        }
    }

    /// 获取addr
    #[inline]
    pub fn get_addr(&self) -> SocketAddr {
        self.addr
    }

    /// 获取conv
    #[inline]
    pub fn get_conv(&self) -> u32 {
        self.conv
    }

    /// 读取数据包
    #[inline]
    pub async fn recv(&self, buf: &mut [u8]) -> KcpResult<usize> {
        /// 用于等待读取
        struct WaitInput<'a> {
            /// kcp peer
            peer: &'a KcpPeer,
            /// 状态机
            state: WaitInputState<'a>,
        }

        /// 等待读取状态
        enum WaitInputState<'a> {
            /// 检查
            Check,
            /// 第一次 解锁 kcp
            WaitLock(BoxFuture<'a, MutexGuard<'a, Kcp>>),
        }

        impl<'a> Future for WaitInput<'a> {
            type Output = KcpResult<usize>;

            #[inline]
            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                loop {
                    match self.state {
                        WaitInputState::Check => {
                            self.state = WaitInputState::WaitLock(self.peer.kcp.lock().boxed());
                            continue;
                        }
                        WaitInputState::WaitLock(ref mut lock_future) => {
                            let kcp = ready!(lock_future.as_mut().poll(cx));
                            let result_size = match kcp.peek_size() {
                                Ok(size) => Ok(size),
                                Err(err) => {
                                    if self.peer.is_broken_pipe.load(Ordering::Acquire) {
                                        //如果udp层关闭那么 返回 BrokenPipe
                                        Err(crate::prelude::kcp_module::Error::BrokenPipe)
                                    } else {
                                        Err(err)
                                    }
                                }
                            };

                            match result_size {
                                Ok(size) => {
                                    if size > 0 {
                                        return Ok(size).into();
                                    }
                                }
                                Err(crate::prelude::kcp_module::Error::BrokenPipe) => {
                                    return Err(crate::prelude::kcp_module::Error::BrokenPipe)
                                        .into()
                                }
                                Err(_) => {
                                    self.peer.wake.register(cx.waker());
                                    self.state = WaitInputState::Check;
                                    return Poll::Pending;
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut wait = WaitInput {
            peer: self,
            state: WaitInputState::Check,
        };
        let size = poll_fn(|cx| Pin::new(&mut wait).poll(cx)).await?;
        if buf.len() < size {
            Err(crate::prelude::kcp_module::Error::UserBufTooSmall(size))
        } else {
            self.recv_buf(buf).await
        }
    }

    /// 发送数据
    #[inline]
    pub async fn send(&self, buf: &[u8]) -> KcpResult<usize> {
        self.kcp.lock().await.send(buf)
    }

    /// flush
    #[inline]
    pub async fn flush(&self) -> KcpResult<()> {
        self.kcp.lock().await.flush_async().await
    }

    /// 获取数据读取器
    #[inline]
    pub fn get_reader<'a>(self: &'a KCPPeer) -> KcpReader<'a> {
        KcpReader::from(self)
    }

    /// 获取写入器
    #[inline]
    pub fn get_writer<'a>(self: &'a KCPPeer) -> KcpWriter<'a> {
        KcpWriter::from(self)
    }
}
