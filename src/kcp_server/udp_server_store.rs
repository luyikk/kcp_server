use super::udp_listener::UdpListener;
use std::cell::RefCell;
use std::sync::Arc;

/// 用来存储 UDP SERVER
pub struct UdpServerStore(pub RefCell<Option<Arc<dyn UdpListener>>>);

unsafe impl Send for UdpServerStore{}
unsafe impl Sync for UdpServerStore{}

impl UdpServerStore {

    /// 获取
    pub fn get(&self) -> Option<Arc<dyn UdpListener>> {
        match *self.0.borrow_mut() {
            None => None,
            Some(ref mut v) => Some(v.clone()),
        }
    }

    /// 设置
    pub fn set(&self, v: Arc<dyn UdpListener>) {
        (*self.0.borrow_mut() )= Some(v);
    }
}
