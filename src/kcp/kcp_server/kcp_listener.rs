use super::super::kcp_module::kcp_config::KcpConfig;
use super::super::kcp_module::Kcp;
use super::buff_input_store::{BuffInputStore, KcpPeerDropInputStore};
use super::kcp_peer::KcpPeer;
use super::kcp_peer_manager::KcpPeerManager;
use super::udp_server_store::UdpServerStore;
use crate::udp::{RecvType, SendUDP, TokenStore, UdpServer};
use async_mutex::Mutex;
use bytes::{BufMut, Bytes, BytesMut};
use log::*;
use std::cell::{RefCell, UnsafeCell};
use std::error::Error;
use std::future::Future;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// KcpListener 整个KCP 服务的入口点
/// config 存放KCP 配置
/// S是用户逻辑上下文类型
pub struct KcpListener<S, R> {
    udp_server: UdpServerStore,
    config: KcpConfig,
    conv_make: AtomicU32,
    buff_input: UnsafeCell<BuffInputStore<S, R>>,
    peers: Arc<KcpPeerManager<S>>,
    peer_kcpdrop_event: Arc<Mutex<KcpPeerDropInputStore>>,
    drop_timeout_second: i64,
}

unsafe impl<S, R> Send for KcpListener<S, R> {}
unsafe impl<S, R> Sync for KcpListener<S, R> {}

impl<S, R> KcpListener<S, R>
where
    S: Send + 'static,
    R: Future<Output = Result<(), Box<dyn Error>>> + Send + 'static,
{
    /// 创建一个KCPListener
    /// addr 监听的本地地址:端口
    /// config 是KCP的配置
    pub async fn new<A: ToSocketAddrs>(
        addr: A,
        config: KcpConfig,
        drop_timeout_second: i64,
    ) -> Result<Arc<Self>, Box<dyn Error>> {
        // 初始化一个kcp_listener
        let kcp_listener = KcpListener {
            udp_server: UdpServerStore(UnsafeCell::new(None)),
            buff_input: UnsafeCell::new(BuffInputStore(None)),
            conv_make: AtomicU32::new(1),
            config,
            peers: Arc::new(KcpPeerManager::new()),
            peer_kcpdrop_event: Arc::new(Mutex::new(KcpPeerDropInputStore::new())),
            drop_timeout_second,
        };

        // 将kcp_listener 放入arc中
        let kcp_listener_arc = Arc::new(kcp_listener);
        // 制造一个UDP SERVER
        let mut udp_serv = UdpServer::new_inner(addr, kcp_listener_arc.clone()).await?;
        //设置数据表输入
        udp_serv.set_input(Self::buff_input);
        //设置错误输出
        udp_serv.set_err_input(Self::err_input);

        //设置删除PEER通知
        let remove_kcp_listener = kcp_listener_arc.clone();
        udp_serv.set_remove_input(move |conv| {
            remove_kcp_listener.peers.remove(&conv);
        });

        //将UDP server 配置到udp_server属性中
        {
            kcp_listener_arc.udp_server.set(Arc::new(udp_serv));
        }
        // 将kcplistener 返回
        Ok(kcp_listener_arc)
    }

    /// 设置数据表输入函数
    pub fn set_buff_input(&self, f: impl Fn(Arc<KcpPeer<S>>, Bytes) -> R + 'static + Send + Sync) {
        let input = self.buff_input.get();
        unsafe {
            (*input).set(Box::new(f));
        }
    }

    /// 设置KCP_PEER 清理事件
    pub async fn set_kcpdrop_event_input(&self, f: fn(u32)) {
        self.peer_kcpdrop_event.lock_arc().await.set(f);
    }

    /// 启动服务
    pub async fn start(&self) -> Result<(), Box<dyn Error>> {
        if let Some(udp_server) = self.udp_server.get() {
            self.update();
            self.cleanup();
            udp_server.start().await?;
        } else {
            return Err("udp_server is nil".into());
        }
        Ok(())
    }

    /// 获取当前时间戳 转换为u32
    #[inline(always)]
    fn current() -> u32 {
        let time = chrono::Local::now().timestamp_millis() & 0xffffffff;
        time as u32
    }

    /// 检查和清除没有用的KCP
    #[inline]
    fn cleanup(&self) {
        let peers = self.peers.clone();
        let clean_input = self.peer_kcpdrop_event.clone();
        let timeout = self.drop_timeout_second;
        if let Some(udp_server) = self.udp_server.get() {
            tokio::spawn(async move {
                let mut remove_vec = vec![];
                let mut remove_peers = vec![];
                loop {
                    for conv in peers.keys() {
                        if let Some(peer) = peers.get(conv) {
                            let time = peer.last_rev_time.load(Ordering::Acquire);
                            if chrono::Local::now().timestamp() - time > timeout {
                                remove_vec.push(*conv);
                                remove_peers.push(peer);
                            }
                        }
                    }

                    if !remove_vec.is_empty() {
                        if let Some(tx) = udp_server.get_msg_tx() {
                            for conv in remove_vec.iter() {
                                if let Some(clean_input) = clean_input.lock_arc().await.get() {
                                    clean_input(*conv);
                                }
                                if let Err(er) = tx.send(RecvType::REMOVE(*conv)).await {
                                    error!("remove error:{:?}", er)
                                }
                            }
                        }
                    }
                    sleep(Duration::from_millis(500)).await;
                    remove_vec.clear();
                    remove_peers.clear();
                }
            });
        }
    }

    /// 刷新KCP
    #[inline]
    fn update(&self) {
        let peers = self.peers.clone();
        tokio::spawn(async move {
            loop {
                let time = Self::current();
                for p in peers.values() {
                    let peer = p.clone();
                    if time >= peer.next_update_time.load(Ordering::Acquire) {
                        let result = peer.update(time).await;
                        if let Err(er) = result {
                            error!("update error:{:?}", er);
                        }
                    }
                }

                //等待5毫秒后再重新UPDATE
                sleep(Duration::from_millis(2)).await;
            }
        });
    }

    /// 异常输入
    /// 打印日志
    #[inline]
    fn err_input(addr: Option<SocketAddr>, err: Box<dyn Error>) -> bool {
        match addr {
            Some(addr) => error!("udp server {} err:{}", addr, err),
            None => error!("udp server err:{}", err),
        }
        false
    }

    /// 生成一个u32的conv
    #[inline]
    fn make_conv(&self) -> u32 {
        let old = self.conv_make.fetch_add(1, Ordering::Release);
        if old == u32::max_value() - 1 {
            self.conv_make.store(1, Ordering::Release);
        }
        old
    }

    /// UDP 数据表输入
    /// 发送回客户端 格式为 [u8;4]+[u8;4] =[u8;8],前面4字节为客户端所发,后面4字节为conv id
    /// 如果不是第一发包 就将数据表压入到 kcp_module,之后读取 数据包输出 真实的数据包结构
    #[inline]
    async fn buff_input(
        this: Arc<Self>,
        sender: SendUDP,
        addr: SocketAddr,
        data: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        if data.len() >= 24 {
            let kcp_peer = Self::get_kcp_peer_and_input(&this, sender, addr, &data).await;
            Self::recv_buff(this, kcp_peer).await?;
        } else if data.len() == 4 {
            // 申请CONV
            Self::make_kcp_peer(this, sender, addr, data).await?;
        } else {
            sender.send((data, addr))?;
        }
        Ok(())
    }

    /// 读取数据包
    #[inline]
    async fn recv_buff(this: Arc<Self>, kcp_peer: Arc<KcpPeer<S>>) -> Result<(), Box<dyn Error>> {
        while let Ok(len) = kcp_peer.peeksize().await {
            let mut buff = vec![0; len];
            if kcp_peer.recv(&mut buff).await.is_ok() {
                let p = this.buff_input.get() as usize;
                if let Some(input) =
                    (*unsafe { std::mem::transmute::<_, &mut BuffInputStore<S, R>>(p) }).get()
                {
                    input(kcp_peer.clone(), Bytes::from(buff)).await?;
                }
            }
        }
        Ok(())
    }

    /// 读取下发的conv,返回kcp_peer 如果在字典类中存在返回kcp_peer
    /// 否则创建一个kcp_peer 绑定到字典类中
    #[inline]
    async fn get_kcp_peer_and_input(
        this: &Arc<Self>,
        sender: SendUDP,
        addr: SocketAddr,
        data: &[u8],
    ) -> Arc<KcpPeer<S>> {
        let mut conv_data = [0; 4];
        conv_data.copy_from_slice(&data[0..4]);
        let conv = u32::from_le_bytes(conv_data);

        let kcp_peer: Arc<KcpPeer<S>> = {
            if let Some(peer) = this.peers.get(&conv) {
                peer
            } else {
                let peer = Self::make_kcp_peer_ptr(conv, sender, addr, this.clone()).await;
                this.peers.insert(conv, peer.clone());
                peer
            }
        };

        if let Err(er) = kcp_peer.input(data).await {
            error!("get_kcp_peer input is err:{}", er);
        }

        kcp_peer
            .last_rev_time
            .store(chrono::Local::now().timestamp(), Ordering::Release);
        kcp_peer
    }

    /// 创建一个KCP_PEER 并存入 Kcp_peers 字典中
    /// 首先判断 是否第一次发包
    /// 如果第一次发包 看看发的是不是 [u8;4] 是的话 生成一个conv id,同时配置一个KcpPeer存储于UDP TOKEN中
    #[inline]
    async fn make_kcp_peer(
        this: Arc<Self>,
        sender: SendUDP,
        addr: SocketAddr,
        data: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {
        // 清除上一次的kcp
        // 创建一个 conv 写入临时连接表
        // 给客户端回复 conv
        let conv = this.make_conv();
        info!("{} make conv:{}", addr, conv);
        //给客户端回复
        let mut buff = BytesMut::new();
        buff.put_slice(&data);
        buff.put_u32_le(conv);
        sender.send((buff.to_vec(), addr))?;
        Ok(())
    }

    /// 创建一个 kcp_peer_ptr
    #[inline]
    async fn make_kcp_peer_ptr(
        conv: u32,
        sender: SendUDP,
        addr: SocketAddr,
        this: Arc<KcpListener<S, R>>,
    ) -> Arc<KcpPeer<S>> {
        let mut kcp = Kcp::new(conv, sender, addr);
        let tx = this.udp_server.get().unwrap().get_msg_tx().unwrap();
        this.config.apply_config(&mut kcp);
        let disconnect_event = move |conv: u32| {
            match this.udp_server.get() {
                Some(udp_server) => {
                    if let Some(tx) = udp_server.get_msg_tx() {
                        let kcpdrop_event = this.peer_kcpdrop_event.clone();
                        tokio::spawn(async move {
                            if let Some(peer) = this.peers.get(&conv) {
                                if let Some(kcpdrop_action) = kcpdrop_event.lock_arc().await.get() {
                                    kcpdrop_action(conv);
                                }
                                if let Err(er) = tx.send(RecvType::REMOVE(conv)).await {
                                    error!("disconnect remove error:{:?}", er)
                                }
                                //等待500毫秒后再清除PEER 以保障UPDATE 的时候PEER 还存在
                                sleep(Duration::from_millis(500)).await;
                                drop(peer); //防优化
                            }
                        });
                    } else {
                        error!("disconnect not get msg tx")
                    }
                }
                None => error!("disconnect not get udp server"),
            }
        };

        let kcp_lock = kcp.get_lock();
        let kcp_peer_obj = KcpPeer {
            kcp: kcp_lock,
            conv,
            addr,
            token: RefCell::new(TokenStore(None)),
            last_rev_time: AtomicI64::new(0),
            next_update_time: AtomicU32::new(0),
            disconnect_event: Mutex::new(Some(Box::new(disconnect_event))),
            main_tx: tx,
        };

        Arc::new(kcp_peer_obj)
    }
}
