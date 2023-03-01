use async_lock::RwLock;
use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::io;
use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use udp_server::prelude::{UDPPeer, UdpServer};

use crate::kcp::kcp_module::prelude::{Kcp, KcpConfig};
use crate::kcp::kcp_server::kcp_peer::{KCPPeer, KcpPeer};
use crate::kcp::kcp_server::udp_sever::IUdpServer;
use crate::prelude::kcp_module::KcpResult;

/// KcpListener 整个KCP 服务的入口点
/// config 存放KCP 配置
/// # example
/// ```ignore
/// use kcpserver::prelude::{
///     kcp_module::{KcpConfig, KcpNoDelayConfig},
///     *,
/// };
/// use tokio::io::AsyncReadExt;
///
/// let mut config = KcpConfig::default();
/// config.nodelay = Some(KcpNoDelayConfig::fastest());
/// let kcp_server = KcpListener::new("0.0.0.0:5555", config, 5, |peer| async move {
///    log::debug!("create kcp peer:{}", peer.to_string());
///    let mut buf = [0; 1024];
///    let mut reader = peer.get_reader();
///    while let Ok(size) = reader.read(&mut buf).await {
///       log::debug!("read peer:{} buff:{}", peer.to_string(), size);
///       peer.send(&buf[..size]).await?;
///    }
///    Ok(())
/// })?;
/// kcp_server.start().await?;
/// ```
pub struct KcpListener<I> {
    udp_server: Box<dyn IUdpServer<Arc<Self>>>,
    config: KcpConfig,
    conv_make: AtomicU32,
    peers: RwLock<HashMap<u32, KCPPeer>>,
    input: I,
}

impl<I, R> KcpListener<I>
where
    I: Fn(KCPPeer) -> R + Send + Sync + 'static,
    R: Future<Output = Result<(), Box<dyn Error>>> + Send + 'static,
{
    /// 创建一个kcp listener
    pub fn new<A: ToSocketAddrs>(
        addr: A,
        config: KcpConfig,
        drop_timeout_second: u64,
        input: I,
    ) -> io::Result<Arc<Self>> {
        let kcp_listener = KcpListener {
            udp_server: Box::new(
                UdpServer::<_, Arc<Self>>::new(
                    addr,
                    |peer, mut reader, kcp_listener| async move {
                        // 根据client数据包结构 进行 kcp peer初始化，
                        // 初始化过程中 任何异常将中断udp peer
                        let (conv, kcp_peer) = loop {
                            if let Some(data) = reader.recv().await {
                                let data = data?;
                                if data.len() >= 24 {
                                    //初始化kcp peer,并跳出 create peer监听逻辑
                                    let mut read = data_rw::DataReader::from(&data);
                                    let conv: u32 = read.read_fixed()?;
                                    read.advance(8)?;
                                    let sn: u32 = read.read_fixed()?;
                                    if sn == 0 {
                                        // 如果收到的conv是分配未使用的
                                        let peer = kcp_listener.create_kcp_peer(conv, &peer);
                                        peer.input(&data).await?;
                                        break (conv, peer);
                                    } else {
                                        // 如果收到的sn是非法的 不理会
                                        log::trace!(
                                            "udp peer:{} conv:{} create kcp error sn:{}",
                                            peer.get_addr(),
                                            conv,
                                            sn
                                        );
                                        continue;
                                    }
                                } else if data.len() == 4 {
                                    // 制造一个conv
                                    kcp_listener.send_kcp_conv(&peer, &data).await?;
                                } else {
                                    //无脑回包
                                    peer.send(&data).await?;
                                }
                            } else {
                                log::error!("udp peer:{} channel is close", peer.get_addr());
                                return Ok(());
                            }
                        };

                        // 触发 kcp peer 主逻辑
                        let peer = kcp_peer.clone();
                        tokio::spawn(async move {
                            kcp_listener.peers.write().await.insert(conv, peer.clone());
                            if let Err(err) = (kcp_listener.input)(peer).await {
                                log::error!("kcp input error:{}", err);
                            }
                            if let Some(peer) = kcp_listener.peers.write().await.remove(&conv) {
                                // 当peer主逻辑关闭，那么将关闭udp,触发udp主逻辑关闭
                                peer.close().await;
                            }
                        });

                        // 读取udp数据包，并写入kcp 直到udp peer 关闭
                        while let Some(Ok(data)) = reader.recv().await {
                            if let Err(err) = kcp_peer.input(&data).await {
                                log::error!("kcp peer input error:{err}");
                                break;
                            }
                        }
                        log::debug!("udp broken kcp peer:{}", kcp_peer.to_string());
                        // 设置 broken pipe 如果kcp主逻辑停留在 recv 那么将立马触发 recv 异常 从而中断 kcp主逻辑
                        kcp_peer.set_broken_pipe();
                        Ok(())
                    },
                )?
                .set_peer_timeout_sec(drop_timeout_second),
            ),
            config,
            conv_make: Default::default(),
            peers: Default::default(),
            input,
        };

        Ok(Arc::new(kcp_listener))
    }

    /// 开始服务
    pub async fn start(self: &Arc<Self>) -> KcpResult<()> {
        let listener = self.clone();
        tokio::task::spawn(async move {
            loop {
                let timestamp = Self::timestamp();
                for peer in listener.peers.read().await.values() {
                    if peer.need_update(timestamp) {
                        let peer = peer.clone();
                        tokio::spawn(async move {
                            if let Err(err) = peer.update(timestamp).await {
                                log::error!("update kcp peer:{} error:{}", peer.to_string(), err)
                            }
                        });
                    }
                }
                //等待至少5毫秒后再重新UPDATE
                sleep(Duration::from_millis(5)).await;
            }
        });

        self.udp_server.start_udp_server(self.clone()).await?;
        Ok(())
    }

    /// 首先判断 是否第一次发包
    /// 如果第一次发包 看看发的是不是 [u8;4] 是的话 生成一个conv id,并记录,然后给客户端发回
    #[inline]
    async fn send_kcp_conv(self: &Arc<Self>, udp_peer: &UDPPeer, data: &[u8]) -> KcpResult<()> {
        let conv = self.make_conv();
        log::trace!("{} make conv:{}", udp_peer.get_addr(), conv);
        let mut buff = data_rw::Data::with_capacity(8);
        buff.write_buf(data);
        buff.write_fixed(conv);
        udp_peer.send(&buff).await?;
        Ok(())
    }

    /// 生成一个u32的conv
    #[inline]
    fn make_conv(&self) -> u32 {
        let old = self.conv_make.fetch_add(1, Ordering::Acquire);
        if old == u32::MAX - 1 {
            self.conv_make.store(1, Ordering::Release);
        }
        old
    }

    /// 创建一个 KCP PEER
    #[inline]
    fn create_kcp_peer(self: &Arc<Self>, conv: u32, udp_peer: &UDPPeer) -> KCPPeer {
        let mut kcp = Kcp::new(conv, udp_peer.clone());
        self.config.apply_config(&mut kcp);
        KcpPeer::new(kcp, conv, udp_peer.get_addr())
    }

    /// 获取当前时间戳 转换为u32
    #[inline(always)]
    fn timestamp() -> u32 {
        let time = chrono::Local::now().timestamp_millis() & 0xffffffff;
        time as u32
    }
}
