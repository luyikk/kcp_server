use super::KCPPeer;
use crate::kcp::kcp_module::prelude::Kcp;
use crate::prelude::kcp_module::KcpResult;
use async_lock::MutexGuard;
use futures::future::BoxFuture;
use futures::FutureExt;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

/// kcp 写入器
/// 同时实现了 tokio AsyncWrite 和 futures AsyncWrite
pub struct KcpWriter<'a> {
    /// kcp peer
    peer: &'a KCPPeer,
    /// kcp 状态机
    state: KcpWriterState<'a>,
}

/// 写入器状态机
enum KcpWriterState<'a> {
    /// 开始
    Begin,
    /// 写入状态
    Write(BoxFuture<'a, MutexGuard<'a, Kcp>>),
    /// flush状态
    FlushAsync(BoxFuture<'a, KcpResult<()>>),
    /// 关闭状态
    Close(BoxFuture<'a, ()>),
}

impl<'a> From<&'a KCPPeer> for KcpWriter<'a> {
    fn from(peer: &'a KCPPeer) -> Self {
        Self {
            peer,
            state: KcpWriterState::Begin,
        }
    }
}

impl KcpWriter<'_> {
    /// 实现发送
    #[inline]
    fn poll_send(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        loop {
            match self.state {
                KcpWriterState::Begin => {
                    self.state = KcpWriterState::Write(self.peer.kcp.lock().boxed());
                }
                KcpWriterState::Write(ref mut write_lock) => {
                    let mut kcp = ready!(write_lock.as_mut().poll(cx));
                    self.state = KcpWriterState::Begin;
                    return match kcp.send(buf) {
                        Ok(size) => Ok(size).into(),
                        Err(err) => Err(err.into()).into(),
                    };
                }
                _ => panic!("poll_send KcpWriterState error state"),
            }
        }
    }

    /// 实现 flush
    #[inline]
    fn poll_flush_async(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        loop {
            match self.state {
                KcpWriterState::Begin => {
                    self.state = KcpWriterState::FlushAsync(self.peer.flush().boxed());
                }
                KcpWriterState::FlushAsync(ref mut flush_future) => {
                    let res = ready!(flush_future.as_mut().poll(cx));
                    self.state = KcpWriterState::Begin;
                    return match res {
                        Ok(()) => Ok(()).into(),
                        Err(err) => Err(err.into()).into(),
                    };
                }
                _ => panic!("poll_flush_async KcpWriterState error state"),
            }
        }
    }

    /// 实现close
    #[inline]
    fn poll_close_async(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        loop {
            match self.state {
                KcpWriterState::Begin => {
                    self.state = KcpWriterState::Close(self.peer.close().boxed());
                }
                KcpWriterState::Close(ref mut close_future) => {
                    ready!(close_future.as_mut().poll(cx));
                    self.state = KcpWriterState::Begin;
                    return Ok(()).into();
                }
                _ => panic!("poll_close KcpWriterState error state"),
            }
        }
    }
}

impl<'a> futures::AsyncWrite for KcpWriter<'a> {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.poll_send(cx, buf)
    }

    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.poll_flush_async(cx)
    }

    #[inline]
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.poll_close_async(cx)
    }
}

impl<'a> tokio::io::AsyncWrite for KcpWriter<'a> {
    #[inline]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.poll_send(cx, buf)
    }

    #[inline]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.poll_flush_async(cx)
    }

    #[inline]
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.poll_close_async(cx)
    }
}
