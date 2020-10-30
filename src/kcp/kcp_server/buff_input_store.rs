use super::kcp_peer::KcpPeer;
use bytes::Bytes;
use std::error::Error;
use std::future::Future;
use std::sync::Arc;

/// 数据包输入原型
type BuffInputType<S, R> = dyn Fn(Arc<KcpPeer<S>>, Bytes) -> R + 'static + Send + Sync;

/// 用来存储 数据表输入函数
pub struct BuffInputStore<S, R>(pub Option<Box<BuffInputType<S, R>>>);

impl<S: Send + 'static, R: Future<Output = Result<(), Box<dyn Error>>> + Send + 'static>
    BuffInputStore<S, R>
{
    /// 获取
    pub fn get(&self) -> Option<&BuffInputType<S, R>> {
        self.0.as_ref().map(|x| x as _)
    }
    /// 设置
    pub fn set(&mut self, v: Box<BuffInputType<S, R>>) {
        self.0 = Some(v);
    }
}

/// 用于通知清理KCP_PEER 事件
pub struct KcpPeerDropInputStore(pub Option<fn(u32)>);

unsafe impl Send for KcpPeerDropInputStore {}
unsafe impl Sync for KcpPeerDropInputStore {}

impl KcpPeerDropInputStore {
    pub fn new() -> KcpPeerDropInputStore {
        KcpPeerDropInputStore(None)
    }

    /// 获取
    pub fn get(&self) -> Option<fn(u32)> {
        self.0.as_ref().map(|x| *x as _)
    }
    /// 设置
    pub fn set(&mut self, v: fn(u32)) {
        self.0 = Some(v);
    }
}
