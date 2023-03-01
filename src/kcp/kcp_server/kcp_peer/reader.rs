use super::KCPPeer;
use crate::kcp::kcp_module::prelude::Kcp;
use crate::prelude::kcp_module::KcpResult;
use async_lock::RwLockWriteGuard;
use futures::future::BoxFuture;
use futures::{ready, FutureExt};
use std::io::{Error, ErrorKind};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::ReadBuf;

/// kcp 读取器 可以使用tokio 的AsyncRead 套件 以及 futures 的AsyncRead 套件
/// ```ignore
/// let mut reader= KcpReader::from(&kcp_peer);
/// // or
/// let mut reader = peer.get_reader();
/// ```
pub struct KcpReader<'a> {
    /// KCP peer
    peer: &'a KCPPeer,
    /// 零时缓存
    cache: Vec<u8>,
    /// 缓存的实际数据长度
    cache_siz: usize,
    /// 缓存当前偏移
    cache_pos: usize,
    /// 状态机
    state: KcpReaderState<'a>,
}

/// 用于对KcpReader future 的状态机
enum KcpReaderState<'a> {
    /// 开始
    Begin,
    /// 读取需要 的长度信息
    ReadSize(BoxFuture<'a, KcpResult<usize>>),
    /// 读取数据
    Recv {
        cache: bool,
        lock_kcp: BoxFuture<'a, RwLockWriteGuard<'a, Kcp>>,
    },
}

impl<'a> From<&'a KCPPeer> for KcpReader<'a> {
    fn from(peer: &'a KCPPeer) -> Self {
        Self {
            peer,
            cache: vec![],
            cache_siz: 0,
            cache_pos: 0,
            state: KcpReaderState::Begin,
        }
    }
}

impl KcpReader<'_> {
    /// poll 读取数据包
    /// 如果 需要读取的kcp buf 大于 buf的长度
    /// 那么 就先读取到cache中
    /// 然后在copy到buf
    /// 否则 就直接 读取到buf中
    #[inline]
    fn poll_recv(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<std::io::Result<usize>> {
        loop {
            match self.state {
                KcpReaderState::Begin => {
                    if self.cache_pos < self.cache_siz {
                        let copy_size = (self.cache_siz - self.cache_pos).min(buf.len());
                        buf[..copy_size].copy_from_slice(
                            &self.cache[self.cache_pos..self.cache_pos + copy_size],
                        );
                        self.cache_pos += copy_size;
                        return Ok(copy_size).into();
                    }
                    self.state = KcpReaderState::ReadSize(self.peer.peek_size().boxed());
                }
                KcpReaderState::ReadSize(ref mut size_future) => {
                    match ready!(size_future.as_mut().poll(cx)) {
                        Ok(0) => return Ok(0).into(),
                        Ok(size) => {
                            if size > buf.len() {
                                if self.cache.len() < size {
                                    self.cache.resize(size, 0);
                                }
                                self.state = KcpReaderState::Recv {
                                    cache: true,
                                    lock_kcp: self.peer.kcp.write().boxed(),
                                }
                            } else {
                                self.state = KcpReaderState::Recv {
                                    cache: false,
                                    lock_kcp: self.peer.kcp.write().boxed(),
                                }
                            }
                        }
                        Err(crate::prelude::kcp_module::Error::BrokenPipe) => {
                            return Err(Error::new(
                                ErrorKind::BrokenPipe,
                                "kcp peer is broken pipe",
                            ))
                            .into()
                        }
                        Err(_) => {
                            // 如果是个错误 基本上是 RecvQueueEmpty
                            // 那么 将开始异步等待
                            self.peer.wake.register(cx.waker());
                            self.state = KcpReaderState::Begin;
                            return Poll::Pending;
                        }
                    }
                }
                KcpReaderState::Recv {
                    cache,
                    ref mut lock_kcp,
                } => {
                    let mut kcp = ready!(lock_kcp.as_mut().poll(cx));
                    if cache {
                        // 如果读取到cache
                        // 成功的话 将设置 cache_siz 和 cache_pos
                        match kcp.recv(&mut self.cache) {
                            Ok(0) => return Ok(0).into(),
                            Ok(size) => {
                                self.cache_siz = size;
                                self.cache_pos = 0;
                                self.state = KcpReaderState::Begin;
                            }
                            Err(err) => return Err(err.into()).into(),
                        }
                    } else {
                        match kcp.recv(buf) {
                            Ok(size) => return Ok(size).into(),
                            Err(crate::prelude::kcp_module::Error::RecvQueueEmpty) => {
                                self.state = KcpReaderState::Begin;
                            }
                            Err(crate::prelude::kcp_module::Error::UserBufTooSmall(size)) => {
                                if self.cache.len() < size {
                                    self.cache.resize(size, 0);
                                }
                                self.state = KcpReaderState::Recv {
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

/// 实现 futures 套件
impl<'a> futures::AsyncRead for KcpReader<'a> {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let poll = ready!(self.poll_recv(cx, buf));
        //凡是正常返回 将重置状态机状态到开始
        self.state = KcpReaderState::Begin;
        poll.into()
    }
}

/// 实现tokio 套件
impl<'a> tokio::io::AsyncRead for KcpReader<'a> {
    #[inline]
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let res = ready!(self.poll_recv(cx, buf.initialize_unfilled()));
        //凡是正常返回 将重置状态机状态到开始
        self.state = KcpReaderState::Begin;
        match res {
            Ok(size) => {
                buf.advance(size);
                Ok(()).into()
            }
            Err(err) => Err(err).into(),
        }
    }
}
