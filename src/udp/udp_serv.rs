use super::error;
use super::send::{SendPool, SendUDP};
use net2::{UdpBuilder, UdpSocketExt};
use std::cell::{RefCell, UnsafeCell};
use std::convert::TryFrom;
use std::error::Error;
use std::future::Future;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{channel, Sender, UnboundedSender};
use tokio::sync::Mutex;

#[cfg(not(target_os = "windows"))]
use net2::unix::UnixUdpBuilderExt;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;

/// 用于通知主线程具体工作
pub enum RecvType {
    INPUT(UnboundedSender<(Vec<u8>, SocketAddr)>, SocketAddr, Vec<u8>),
    REMOVE(u32),
}

impl Debug for RecvType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RecvType::INPUT(_, _, _) => f.debug_struct("INPUT").finish(),
            RecvType::REMOVE(_) => f.debug_struct("REMOVE").finish(),
        }
    }
}

/// UDP 单个包最大大小,默认4096 主要看MTU一般不会超过1500在internet 上
/// 如果局域网有可能大点,4096一般够用.
pub const BUFF_MAX_SIZE: usize = 4096;

/// UDP SOCKET 上下文,包含 id, 和Pees, 用于收发数据
pub struct UdpContext {
    pub id: usize,
    recv: RefCell<Option<Arc<UdpSocket>>>,
    pub send: SendPool,
}

unsafe impl Send for UdpContext {}
unsafe impl Sync for UdpContext {}

/// 错误输入类型
pub type ErrorInput = Arc<Mutex<dyn Fn(Option<SocketAddr>, Box<dyn Error>) -> bool + Send>>;

/// UDP 服务器对象
/// I 用来限制必须input的FN 原型,
/// R 用来限制 必须input的是 异步函数
/// T 用来设置返回值

pub struct UdpServer<I, R, S> {
    inner: Arc<S>,
    udp_contexts: Vec<UdpContext>,
    input: Option<Arc<I>>,
    error_input: Option<ErrorInput>,
    remove_event: Option<Box<dyn Fn(u32)>>,
    msg_tx: UnsafeCell<Option<Sender<RecvType>>>,
    phantom:PhantomData<R>
}

unsafe impl<I, R, S> Send for UdpServer<I, R, S> {}

unsafe impl<I, R, S> Sync for UdpServer<I, R, S> {}

/// 用来存储Token
#[derive(Debug)]
pub struct TokenStore<T>(pub Option<T>);

impl<T: Send> TokenStore<T> {
    pub fn have(&self) -> bool {
        self.0.is_some()
    }

    pub fn get(&mut self) -> Option<&mut T> {
        self.0.as_mut()
    }

    pub fn set(&mut self, v: Option<T>) {
        self.0 = v;
    }
}

/// Peer 对象
/// 用来标识client
/// socket_id 标识 所在哪个UDPContent,一般只在头一次接收到数据的时候设置
/// 一般用来所在Peer位置,linux 服务器上启用了 reuse_port
/// 所以后续收到数据包不一定来自于此 所在哪个UDPContent
/// 而windows上只有一个socket,所以始终只有1个
/// # token
/// 你可以自定义放一些和用户有关的数据,这样的话方便你对用户进行区别,已提取用户逻辑数据
#[derive(Debug)]
pub struct Peer<T> {
    pub socket_id: usize,
    pub addr: SocketAddr,
    pub token: Arc<Mutex<TokenStore<T>>>,
    pub udp_sock: SendUDP,
}

impl<T: Send> Peer<T> {
    /// Send 发送数据包
    /// 作为最基本的函数之一,它采用了tokio的async send_to
    /// 首先,他会去弱指针里面拿到强指针,如果没有他会爆错
    pub async fn send(&self, data: Vec<u8>) -> Result<(), Box<dyn Error>> {
        self.udp_sock.send((data, self.addr))?;
        Ok(())
    }
}

impl<I, R> UdpServer<I, R, ()>
where
    I: Fn(Arc<()>, SendUDP, SocketAddr, Vec<u8>) -> R + Send + Sync + 'static,
    R: Future<Output = Result<(), Box<dyn Error>>> + Send,
{
    pub async fn new<A: ToSocketAddrs>(addr: A) -> Result<Self, Box<dyn Error>> {
        Self::new_inner(addr, Arc::new(())).await
    }
}

impl<I, R, S> UdpServer<I, R, S>
where
    I: Fn(Arc<S>, SendUDP, SocketAddr, Vec<u8>) -> R + Send + Sync + 'static,
    R: Future<Output = Result<(), Box<dyn Error>>> + Send,
    S: Sync + Send + 'static,
{
    ///用于非windows 创建socket,和windows的区别在于开启了 reuse_port
    #[cfg(not(target_os = "windows"))]
    fn make_udp_client<A: ToSocketAddrs>(addr: &A) -> Result<std::net::UdpSocket, Box<dyn Error>> {
        let res = UdpBuilder::new_v4()?
            .reuse_address(true)?
            .reuse_port(true)?
            .bind(addr)?;
        Ok(res)
    }

    ///用于windows创建socket
    #[cfg(target_os = "windows")]
    fn make_udp_client<A: ToSocketAddrs>(addr: &A) -> Result<std::net::UdpSocket, Box<dyn Error>> {
        let res = UdpBuilder::new_v4()?.reuse_address(true)?.bind(addr)?;
        Ok(res)
    }

    ///创建udp socket,并设置buffer 大小
    fn create_udp_socket<A: ToSocketAddrs>(
        addr: &A,
    ) -> Result<std::net::UdpSocket, Box<dyn Error>> {
        let res = Self::make_udp_client(addr)?;
        res.set_send_buffer_size(1784 * 10000)?;
        res.set_recv_buffer_size(1784 * 10000)?;
        Ok(res)
    }

    /// 创建tokio的udpsocket ,从std 创建
    fn create_async_udp_socket<A: ToSocketAddrs>(addr: &A) -> Result<UdpSocket, Box<dyn Error>> {
        let std_sock = Self::create_udp_socket(&addr)?;
        std_sock.set_nonblocking(true)?;
        let sock = UdpSocket::try_from(std_sock)?;
        Ok(sock)
    }

    /// 创建tikio UDPClient
    /// listen_count 表示需要监听多少份的UDP SOCKET
    fn create_udp_socket_list<A: ToSocketAddrs>(
        addr: &A,
        listen_count: usize,
    ) -> Result<Vec<UdpSocket>, Box<dyn Error>> {
        println!("cpus:{}", listen_count);
        let mut listens = vec![];
        for _ in 0..listen_count {
            let sock = Self::create_async_udp_socket(addr)?;
            listens.push(sock);
        }
        Ok(listens)
    }

    #[cfg(not(target_os = "windows"))]
    fn get_cpu_count() -> usize {
        num_cpus::get()
    }

    #[cfg(target_os = "windows")]
    fn get_cpu_count() -> usize {
        1
    }

    /// 创建UdpServer
    /// 如果是linux 是系统,他会根据CPU核心数创建等比的UDP SOCKET 监听同一端口
    /// 已达到 M级的DPS 数量
    pub async fn new_inner<A: ToSocketAddrs>(
        addr: A,
        inner: Arc<S>,
    ) -> Result<Self, Box<dyn Error>> {
        let udp_list = Self::create_udp_socket_list(&addr, Self::get_cpu_count())?;
        let mut udp_map = vec![];
        let mut id = 1;
        for udp in udp_list {
            let udp_socket_ptr = Arc::new(udp);
            udp_map.push(UdpContext {
                id,
                recv: RefCell::new(Some(udp_socket_ptr.clone())),
                send: SendPool::new(udp_socket_ptr),
            });
            id += 1;
        }

        Ok(UdpServer {
            inner,
            udp_contexts: udp_map,
            input: None,
            error_input: None,
            msg_tx: UnsafeCell::new(None),
            remove_event: None,
            phantom:PhantomData::default()
        })
    }

    /// 获取消息通知tx
    pub fn get_msg_tx(&self) -> Option<Sender<RecvType>> {
        unsafe {
            if let Some(ref tx) = *self.msg_tx.get() {
                return Some(tx.clone());
            }
            None
        }
    }

    /// 设置收包函数
    /// 此函数必须符合 async 模式
    pub fn set_input(&mut self, input: I) {
        self.input = Some(Arc::new(input));
    }

    /// 设置错误输出
    pub fn set_err_input<P: Fn(Option<SocketAddr>, Box<dyn Error>) -> bool + Send + 'static>(
        &mut self,
        err_input: P,
    ) {
        self.error_input = Some(Arc::new(Mutex::new(err_input)));
    }

    /// 设置删除PEER 通知
    pub fn set_remove_input<F: Fn(u32) + Send + 'static>(&mut self, remove_input: F) {
        self.remove_event = Some(Box::new(remove_input));
    }

    /// 启动服务
    /// 如果input 发生异常,将会发生错误
    /// 这个时候回触发 err_input, 如果没有使用 set_err_input 设置错误回调
    /// 那么 就会输出默认的 err_input,如果输出默认的 err_input 那么整个服务将会停止
    /// 所以如果不想服务停止,那么必须自己实现 err_input 并且返回 false
    pub async fn start(&self) -> Result<(), Box<dyn Error>> {
        if let Some(ref input) = self.input {
            let err_input = {
                if let Some(err) = &self.error_input {
                    let x = err;
                    x.clone()
                } else {
                    Arc::new(Mutex::new(
                        |addr: Option<SocketAddr>, err: Box<dyn Error>| {
                            match addr {
                                Some(addr) => {
                                    println!("{}-{}", addr, err);
                                }
                                None => {
                                    println!("{}", err);
                                }
                            }
                            true
                        },
                    ))
                }
            };
            let (tx, mut rx) = channel(self.udp_contexts.len() * 2048);
            for udp_sock in self.udp_contexts.iter() {
                let recv_sock = udp_sock.recv.borrow_mut().take();
                let send_sock = udp_sock.send.get_tx();
                if let Some(recv_sock) = recv_sock {
                    let move_data_tx = tx.clone();
                    let error_input = err_input.clone();
                    tokio::spawn(async move {
                        let mut buff = [0; BUFF_MAX_SIZE];
                        loop {
                            match recv_sock.recv_from(&mut buff).await{
                                Ok((size, addr)) => {
                                    if let Err(er) = move_data_tx.send(RecvType::INPUT(
                                            send_sock.clone(),
                                            addr,
                                            buff[..size].to_vec(),
                                        )).await
                                    {
                                        let error = error_input.lock().await;
                                        let _ = error(Some(addr), Box::new(er));
                                        break;
                                    }
                                },
                                Err(er) => {
                                    let error = error_input.lock().await;
                                    let stop = error(None, error::Error::IOError(er).into());
                                    if stop {
                                        return;
                                    }
                                }
                            }

                        }
                    });
                }
            }
            unsafe {
                (*self.msg_tx.get()).replace(tx);
            }
            while let Some(recv_type) = rx.recv().await {
                match recv_type {
                    RecvType::INPUT(send_sock, addr, data) => {
                        let err = {
                            let res = input(self.inner.clone(), send_sock, addr, data).await;
                            match res {
                                Err(er) => Some(format!("{}->{:?}", er, er)),
                                Ok(_) => None,
                            }
                        };
                        if let Some(er_msg) = err {
                            let error = err_input.lock().await;
                            let stop = error(Some(addr), er_msg.into());
                            if stop {
                                break;
                            }
                        }
                    }
                    RecvType::REMOVE(ids) => {
                        if let Some(ref remove_input) = self.remove_event {
                            remove_input(ids);
                        }
                    }
                }
            }

            Ok(())
        } else {
            panic!("not found input")
        }
    }
}
