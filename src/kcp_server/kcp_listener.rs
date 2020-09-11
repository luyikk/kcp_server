use std::sync::Arc;
use tokio::sync::Mutex;
use super::UdpServerStore;
use std::sync::atomic::{AtomicU32, Ordering, AtomicI64};
use crate::kcp_server::kcp_peer::KcpPeer;
use crate::kcp_server::buff_input_store::BuffInputStore;
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::time::Duration;
use crate::kcp_server::kcp_config::KcpConfig;
use std::cell::{UnsafeCell, RefCell};
use std::error::Error;
use udp_server::{UdpServer, Peer, TokenStore};
use log::*;
use bytes::{Bytes, BytesMut, BufMut};
use crate::kcp::Kcp;
use tokio::time::delay_for;
use std::future::Future;

/// KcpListener 整个KCP 服务的入口点
/// config 存放KCP 配置
/// S是用户逻辑上下文类型
/// # Examples Echo
/// ```rust
/// #![feature(async_closure)]
/// use kcp_server::KcpListener;
/// use kcp_server::KcpConfig;
/// use kcp_server::KcpNoDelayConfig;
/// use std::error::Error;
///
/// #[tokio::main]
/// async fn main()->Result<(),Box<dyn Error>>{
///      let mut config = KcpConfig::default();
///      config.nodelay = Some(KcpNoDelayConfig::fastest());
///      let kcp = KcpListener::<i32, _>::new("0.0.0.0:5555", config).await?;
///      kcp.set_buff_input(async move |peer, data| {
///          peer.send(&data).await?;
///          Ok(())
///       }).await;
///      kcp.start().await?;
///      Ok(())
/// }
///
///
/// ```
pub struct KcpListener<S,R>
    where S: Send + 'static,
          R: Future<Output = Result<(), Box<dyn Error>>> + Send+'static{
    udp_server: UdpServerStore,
    config: KcpConfig,
    conv_make: AtomicU32,
    buff_input: UnsafeCell<BuffInputStore<S,R>>,
    peers: Arc<Mutex<HashMap<u32, Arc<KcpPeer<S>>>>>
}

unsafe impl<S,R>  Send for KcpListener<S,R> where S: Send + 'static,
                                                    R: Future<Output = Result<(), Box<dyn Error>>> + Send+'static{}
unsafe impl<S,R> Sync for KcpListener<S,R> where S: Send + 'static,
                                                  R: Future<Output = Result<(), Box<dyn Error>>> + Send+'static{}

impl<S,R> KcpListener<S,R>
            where S: Send + 'static,
                  R: Future<Output = Result<(), Box<dyn Error>>> + Send+'static{

    /// 创建一个KCPListener
    /// addr 监听的本地地址:端口
    /// config 是KCP的配置
    pub async fn new<A: ToSocketAddrs>(
        addr: A,
        config: KcpConfig,
    ) -> Result<Arc<Self>, Box<dyn Error>> {
        // 初始化一个kcp_listener
        let kcp_listener = KcpListener {
            udp_server: UdpServerStore(RefCell::new(None)),
            buff_input: UnsafeCell::new(BuffInputStore(None)),
            conv_make: AtomicU32::new(1),
            config,
            peers: Arc::new(Mutex::new(HashMap::new()))
        };

        // 将kcp_listener 放入arc中
        let kcp_listener_arc = Arc::new(kcp_listener);
        // 制造一个UDP SERVER
        let mut udp_serv =
            UdpServer::<_, _, (), _>::new_inner(addr, kcp_listener_arc.clone()).await?;
        //设置数据表输入
        udp_serv.set_input(Self::buff_input);
        //设置错误输出
        udp_serv.set_err_input(Self::err_input);

        //将UDP server 配置到udp_server属性中
        {
            kcp_listener_arc.udp_server.set(Arc::new(udp_serv));
        }
        // 将kcplistener 返回
        Ok(kcp_listener_arc)
    }

    /// 设置数据表输入函数
    pub async fn set_buff_input(
        &self, f: impl Fn(Arc<KcpPeer<S>>, Bytes) ->R
        + 'static
        + Send
        + Sync) {
        let input = self.buff_input.get();
        unsafe {
            (*input).set(Box::new(f));
        }
    }


    /// 启动服务
    pub async fn start(&self) -> Result<(), Box<dyn Error>> {

        if let Some(udp_server) =self.udp_server.get() {
            self.update();
            self.cleanup();
            return udp_server.start();
        }

        Err("udp_server is nil".into())
    }

    /// 获取当前时间戳 转换为u32
    #[inline]
    fn current() -> u32 {
        let time =chrono::Local::now().timestamp_millis() & 0xffffffff;
        time as u32
    }

    /// 检查和清除没有用的KCP
    fn cleanup(&self){
        let peers=self.peers.clone();
        if let Some(udp_server)=self.udp_server.get() {
            tokio::spawn(async move {
                loop {
                    let res = peers.try_lock();
                    if let Ok(mut peers) = res {
                        let mut remove_vec = vec![];

                        for conv in peers.keys() {
                            if let Some(peer) = peers.get(conv) {
                                let time = peer.last_rev_time.load(Ordering::Acquire);
                                if chrono::Local::now().timestamp() - time > 30 {
                                    udp_server.remove_peer(peer.addr);
                                    remove_vec.push(*conv);
                                }
                            }
                        }

                        for conv in remove_vec {
                            peers.remove(&conv);
                        }
                    }

                    delay_for(Duration::from_millis(500)).await;
                }
            });
        }
    }


    /// 刷新KCP
    fn update(&self) {
        let peers=self.peers.clone();
        tokio::spawn( async move {
            loop {
                let res = peers.try_lock();
                if let Ok(mut peers) = res {
                    for (_, p) in peers.iter_mut()
                    {
                        let peer = p.clone();
                        let time=Self::current();
                        tokio::spawn(async move {
                            if let Err(err) =peer.update(time).await{
                                error!("update error:{}", err);
                            }
                        });
                    }
                }
                //等待5毫秒后再重新UPDATE
                delay_for(Duration::from_millis(5)).await;
            }
        });
    }

    /// 异常输入
    /// 打印日志
    #[inline]
    fn err_input(peer: Option<Arc<Peer<()>>>, err: Box<dyn Error>) -> bool {
        match peer {
            Some(peer) => error!("udp server {} err:{}",peer.addr, err),
            None => error!("udp server err:{}", err)
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
   /// 如果不是第一发包 就将数据表压入到 kcp,之后读取 数据包输出 真实的数据包结构
    async fn buff_input(
        this: Arc<Self>,
        peer: Arc<Peer<()>>,
        data: Vec<u8>,
    ) -> Result<(), Box<dyn Error>> {

       if data.len()==4{
           // 申请CONV
           Self::make_kcp_peer(this,peer,data).await?;
       }
       else if data.len()>=24{
           let  kcp_peer=Self::get_kcp_peer(&this,peer.clone(),&data).await;
           match kcp_peer {
               Some(kcp_peer)=> {
                   Self::recv_kcp_buff(this,kcp_peer, data).await?;
               },
               None=>{
                   error!("not found kcp_peer {}",peer.addr);
               }
           }

       }
       else{
           peer.send(&data).await?;
       }
       Ok(())
   }


    /// 将数据包写入2
    #[inline]
    async fn recv_kcp_buff( this: Arc<Self>,kcp_peer:Arc<KcpPeer<S>>, data:Vec<u8>) ->Result<(), Box<dyn Error>> {
        kcp_peer.input(&data).await?;
        Self::recv_buff(this,kcp_peer).await?;
        Ok(())
    }

    /// 读取数据包
    #[inline]
    async fn recv_buff(this:Arc<Self>, kcp_peer: Arc<KcpPeer<S>>)->Result<(), Box<dyn Error>> {

        while let Ok(len) = kcp_peer.peeksize().await {
            let mut buff = vec![0; len];
            if let Ok(_) = kcp_peer.recv(&mut buff).await {
                let p=this.buff_input.get() as usize;
                if let Some(input)=(*unsafe{std::mem::transmute::<_,&mut BuffInputStore<S,R>>(p)}).get(){
                    input(kcp_peer.clone(),Bytes::from(buff)).await?;
                }
            }
        }
        Ok(())
    }



    /// 读取下发的conv,返回kcp_peer 如果在字典类中存在返回kcp_peer
    /// 否则创建一个kcp_peer 绑定到字典类中
    #[inline]
    async fn get_kcp_peer(this: &Arc<Self>,peer: Arc<Peer<()>>,data: &Vec<u8>) -> Option<Arc<KcpPeer<S>>> {
        let mut conv_data = [0; 4];
        conv_data.copy_from_slice(&data[0..4]);
        let conv = u32::from_le_bytes(conv_data);


        let mut peers_lock = this.peers.lock().await;
        match peers_lock.get(&conv) {
            Some(kcp_peer) => {
                kcp_peer.last_rev_time.store(chrono::Local::now().timestamp(), Ordering::Release);
                if let Err(er) = kcp_peer.input(data).await {
                    error!("get_kcp_peer input is err:{}", er);
                }
                return Some(kcp_peer.clone())
            },
            None => {
                let res = Self::make_kcp_peer_ptr(conv, data, peer.clone(), this.clone()).await;
                if let Ok(kcp_peer_arc) = res {
                    let kcp_peer = kcp_peer_arc.clone();
                    peers_lock.insert(conv, kcp_peer.clone());
                    return Some(kcp_peer);
                }
            }
        }

        None
    }

    /// 创建一个KCP_PEER 并存入 Kcp_peers 字典中
    /// 首先判断 是否第一次发包
    /// 如果第一次发包 看看发的是不是 [u8;4] 是的话 生成一个conv id,同时配置一个KcpPeer存储于UDP TOKEN中
    #[inline]
    async fn make_kcp_peer(this: Arc<Self>, peer: Arc<Peer<()>>, data: Vec<u8>) -> Result<(), Box<dyn Error>> {
        // 清除上一次的kcp
        // 创建一个 conv 写入临时连接表
        // 给客户端回复 conv
        let conv = this.make_conv();
        info!("{} make conv:{}", peer.addr, conv);
        //给客户端回复
        let mut buff = BytesMut::new();
        buff.put_slice(&data);
        buff.put_u32_le(conv);
        peer.send(&buff).await?;
        Ok(())
    }

    /// 创建一个 kcp_peer_ptr
    #[inline]
    async fn make_kcp_peer_ptr(conv:u32,buff:&Vec<u8>,peer:Arc<Peer<()>>,this_ptr:Arc<KcpListener<S,R>>)-> Result<Arc<KcpPeer<S>>, Box<dyn Error>>{
        let mut kcp = Kcp::new(conv, peer.udp_sock.clone());
        this_ptr.config.apply_config(&mut kcp);
        let kcp_lock= kcp.get_lock();
        kcp_lock.input(buff).await?;

       let  kcp_peer_obj = KcpPeer {
            kcp:kcp_lock,
            conv,
            addr: peer.addr,
            token: Mutex::new(TokenStore(None)),
            last_rev_time: AtomicI64::new(chrono::Local::now().timestamp())
        };

        let kcp_peer_ptr=Arc::new(kcp_peer_obj);
        Ok(kcp_peer_ptr)
    }


}