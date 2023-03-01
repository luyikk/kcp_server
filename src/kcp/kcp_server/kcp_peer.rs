use crate::kcp::kcp_module::prelude::{Kcp, KcpResult};
use async_lock::RwLockWriteGuard;
use futures::{
    future::{poll_fn, BoxFuture},
    AsyncRead, FutureExt,
};
use std::fmt::Formatter;
use std::future::Future;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::task::{ready, Context, Poll};

pub type KCPPeer = Arc<KcpPeer>;

/// kcp peer 用于管理玩家上下文 以及 读取 和发送 数据
pub struct KcpPeer {
    kcp: async_lock::RwLock<Kcp>,
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
            kcp: async_lock::RwLock::new(kcp),
            wake: Default::default(),
            is_broken_pipe: Default::default(),
            conv,
            addr,
            next_update_time: Default::default(),
        })
    }

    /// 查看能读取多少KCP数据包
    #[inline]
    pub(crate) async fn peek_size(&self) -> KcpResult<usize> {
        match self.kcp.read().await.peek_size() {
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

    /// 往kcp 压udp数据包
    #[inline]
    pub(crate) async fn input(&self, buf: &[u8]) -> KcpResult<usize> {
        match self.kcp.write().await.input(buf) {
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
            self.kcp.read().await.output.peer.close();
        }
    }

    /// 从kcp读取数据包
    #[inline]
    pub(crate) async fn recv_buf(&self, buf: &mut [u8]) -> KcpResult<usize> {
        self.kcp.write().await.recv(buf)
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
            let mut kcp = self.kcp.write().await;
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
            peer: &'a KcpPeer,
            state: WaitInputState<'a>,
        }

        /// 等待读取状态
        enum WaitInputState<'a> {
            /// 检查
            Check,
            /// 第一次 解锁 kcp
            WaitLockOne(BoxFuture<'a, KcpResult<usize>>),
            /// 第二次 解锁 kcp
            WaitLockTow(BoxFuture<'a, KcpResult<usize>>),
        }

        impl<'a> Future for WaitInput<'a> {
            type Output = KcpResult<usize>;

            #[inline]
            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                loop {
                    match self.state {
                        WaitInputState::Check => {
                            self.state = WaitInputState::WaitLockOne(self.peer.peek_size().boxed());
                            continue;
                        }
                        WaitInputState::WaitLockOne(ref mut lock_future) => {
                            match ready!(lock_future.as_mut().poll(cx)) {
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
                                    self.state =
                                        WaitInputState::WaitLockTow(self.peer.peek_size().boxed());
                                }
                            }
                        }
                        WaitInputState::WaitLockTow(ref mut lock_future) => {
                            match ready!(lock_future.as_mut().poll(cx)) {
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
        self.kcp.write().await.send(buf)
    }
}

pub struct KcpStream<'a> {
    peer: &'a KCPPeer,
    cache: Vec<u8>,
    cache_siz: usize,
    cache_pos: usize,
    state: KcpStreamState<'a>,
}

enum KcpStreamState<'a> {
    Begin,
    ReadSize(BoxFuture<'a, KcpResult<usize>>),
    Recv {
        cache: bool,
        lock_kcp: BoxFuture<'a, RwLockWriteGuard<'a, Kcp>>,
    },
}

impl<'a> From<&'a KCPPeer> for KcpStream<'a> {
    fn from(peer: &'a KCPPeer) -> Self {
        Self {
            peer,
            cache: vec![],
            cache_siz: 0,
            cache_pos: 0,
            state: KcpStreamState::Begin,
        }
    }
}

impl KcpStream<'_> {
    fn poll_recv(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<std::io::Result<usize>> {
        loop {
            match self.state {
                KcpStreamState::Begin => {
                    if self.cache_pos < self.cache_siz {
                        let copy_size = (self.cache_siz - self.cache_pos).min(buf.len());
                        buf[..copy_size].copy_from_slice(
                            &self.cache[self.cache_pos..self.cache_pos + copy_size],
                        );
                        self.cache_pos += copy_size;
                        return Ok(copy_size).into();
                    }
                    self.state = KcpStreamState::ReadSize(self.peer.peek_size().boxed());
                }
                KcpStreamState::ReadSize(ref mut size_future) => {
                    match ready!(size_future.as_mut().poll(cx)) {
                        Ok(0) => return Ok(0).into(),
                        Ok(size) => {
                            if size > buf.len() {
                                if self.cache.len() < size {
                                    self.cache.resize(size, 0);
                                }
                                self.state = KcpStreamState::Recv {
                                    cache: true,
                                    lock_kcp: self.peer.kcp.write().boxed(),
                                }
                            } else {
                                self.state = KcpStreamState::Recv {
                                    cache: false,
                                    lock_kcp: self.peer.kcp.write().boxed(),
                                }
                            }
                        }
                        Err(crate::prelude::kcp_module::Error::BrokenPipe) => {
                            return Err(std::io::Error::new(
                                ErrorKind::BrokenPipe,
                                "kcp peer is broken pipe",
                            ))
                            .into()
                        }
                        Err(_) => {
                            self.peer.wake.register(cx.waker());
                            self.state=KcpStreamState::Begin;
                            return Poll::Pending;
                        }
                    }
                }
                KcpStreamState::Recv {
                    cache,
                    ref mut lock_kcp,
                } => {
                    let mut kcp = ready!(lock_kcp.as_mut().poll(cx));
                    if cache {
                        match kcp.recv(&mut self.cache) {
                            Ok(0) => return Ok(0).into(),
                            Ok(size) => {
                                self.cache_siz = size;
                                self.cache_pos = 0;
                                self.state = KcpStreamState::Begin;
                            }
                            Err(err) => return Err(err.into()).into(),
                        }
                    } else {
                        match kcp.recv(buf) {
                            Ok(size) => return Ok(size).into(),
                            Err(crate::prelude::kcp_module::Error::RecvQueueEmpty) => {
                                self.state = KcpStreamState::Begin;
                            }
                            Err(crate::prelude::kcp_module::Error::UserBufTooSmall(size)) => {
                                if self.cache.len() < size {
                                    self.cache.resize(size, 0);
                                }
                                self.state = KcpStreamState::Recv {
                                    cache: true,
                                    lock_kcp: self.peer.kcp.write().boxed(),
                                }
                            }
                            Err(err) => return Err(err.into()).into(),
                        }
                    }
                }
            }
        }
    }
}

impl<'a> AsyncRead for KcpStream<'a> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let poll = ready!(self.poll_recv(cx, buf));
        self.state = KcpStreamState::Begin;
        poll.into()
    }
}
