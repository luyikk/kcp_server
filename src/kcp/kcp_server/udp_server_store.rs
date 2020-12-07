use super::udp_listener::UdpListener;
use std::cell::{UnsafeCell};
use std::sync::Arc;

/// 用来存储 UDP SERVER
pub struct UdpServerStore(pub UnsafeCell<Option<Arc<dyn UdpListener>>>);

unsafe impl Send for UdpServerStore {}
unsafe impl Sync for UdpServerStore {}

impl UdpServerStore {
    /// 获取
    pub fn get(&self) -> Option<Arc<dyn UdpListener>> {
        unsafe {
            match *self.0.get() {
                None => None,
                Some(ref mut v) => Some(v.clone()),
            }
        }
    }

    /// 设置 TCPSERVER
    /// 由于只会在服务器启动的时候set 其他的时候只有CLONE ARC 并不会有安全问题
    /// 所以为了性能和方便 使用了UnsafeCell
    pub fn set(&self, v: Arc<dyn UdpListener>) {
        unsafe {
            *self.0.get() = Some(v);
        }
    }
}
