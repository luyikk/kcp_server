use crate::udp::{TokenStore, IOUpdate};
use super::super::kcp_module::{Kcp, KcpResult};
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::net::SocketAddr;
use log::*;
use std::cell::{RefCell};
use std::error::Error;

/// KCP LOCK
/// 将KCP 对象操作完全上锁,以保证内存安全 通知简化调用
pub struct KcpLock(RefCell<Kcp>);
unsafe impl Send for KcpLock{}
unsafe impl Sync for KcpLock{}

impl KcpLock{
    #[inline(always)]
    pub fn peeksize(&self)-> KcpResult<usize>{
        self.0.borrow_mut().peeksize()
    }

    #[inline(always)]
    pub fn check(&self,current:u32)->u32{
        self.0.borrow_mut().check(current)
    }

    #[inline(always)]
    pub fn input(&self, buf: &[u8]) -> KcpResult<usize>{
        self.0.borrow_mut().input(buf)
    }

    #[inline(always)]
    pub fn recv(&self, buf: &mut [u8]) -> KcpResult<usize>{
        self.0.borrow_mut().recv(buf)
    }

    #[inline(always)]
    pub fn send(&self, buf: &[u8]) -> KcpResult<usize>{
        self.0.borrow_mut().send(buf)
    }

    #[inline(always)]
    pub fn update(&self, current: u32) ->  KcpResult<u32>{
        let mut p= self.0.borrow_mut();
        p.update(current)?;
        Ok(p.check(current))
    }

    #[inline(always)]
    pub fn flush(&self) -> KcpResult<()>{
        self.0.borrow_mut().flush()
    }

    #[inline(always)]
    pub fn flush_async(&self)->  KcpResult<()>{
        self.0.borrow_mut().flush_async()
    }
}


impl Kcp{
    #[inline(always)]
    pub fn get_lock(self)->KcpLock{
        let recv =RefCell::new(self);
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
    pub token: RefCell<TokenStore<T>>,
    pub last_rev_time: AtomicI64,
    pub next_update_time:AtomicU32,
    pub disconnect_event: RefCell<Option<Box<dyn FnOnce(u32)>>>
}

impl<T:Send> Drop for KcpPeer<T>{
    #[inline(always)]
    fn drop(&mut self) {
        info!("kcp_peer:{} is Drop",self.conv);
    }
}


unsafe impl<T: Send> Send for KcpPeer<T>{}
unsafe impl<T: Send> Sync for KcpPeer<T>{}

/// 简化KCP PEER 函数
impl <T:Send> KcpPeer<T>{

    #[inline(always)]
    pub fn disconnect(&self) {
        let call_value = self.disconnect_event.borrow_mut().take();
        if let Some(call) = call_value {
            call(self.conv);
        }
    }

    #[inline(always)]
    pub fn peeksize(&self)-> KcpResult<usize>{
        self.kcp.peeksize()
    }

    #[inline(always)]
    pub fn input(&self, buf: &[u8]) -> KcpResult<usize>{
        self.next_update_time.store(0,Ordering::Release);
        self.kcp.input(buf)
    }

    #[inline(always)]
    pub fn recv(&self, buf: &mut [u8]) -> KcpResult<usize>{
        self.kcp.recv(buf)
    }

    #[inline(always)]
    pub fn send(&self, buf: &[u8]) -> KcpResult<usize>{
        self.next_update_time.store(0,Ordering::Release);
        self.kcp.send(buf)
    }

    #[inline(always)]
    pub fn update(&self, current: u32) ->  KcpResult<()>{
        let next= self.kcp.update(current)?;
        Ok(self.next_update_time.store(next+current,Ordering::Release))
    }

    #[inline(always)]
    pub fn flush(&self) -> KcpResult<()>{
        self.kcp.flush()
    }

    #[inline(always)]
    pub fn flush_async(&self)->  KcpResult<()>{
        self.kcp.flush_async()
    }

}


impl<T:Send> IOUpdate for KcpPeer<T>{
    #[inline(always)]
    fn update(&self, current: u32) -> Result<(), Box<dyn Error>> {
        self.update(current)?;
        Ok(())
    }
}