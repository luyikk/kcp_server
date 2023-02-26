use async_lock::RwLock;
use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::io;
use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use udp_server::prelude::{IUdpPeer, UDPPeer, UdpServer};

use crate::kcp::kcp_module::prelude::{Kcp, KcpConfig};
use crate::kcp::kcp_server::kcp_peer::{IKcpPeer, KCPPeer, KcpPeer};
use crate::kcp::kcp_server::udp_sever::IUdpServer;
use crate::prelude::kcp_module::KcpResult;
use crate::prelude::KcpIO;

/// KcpListener 整个KCP 服务的入口点
/// config 存放KCP 配置
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
    ) -> io::Result<Self> {
        let kcp_listener = KcpListener {
            udp_server: Box::new(
                UdpServer::<_, Arc<Self>>::new(
                    addr,
                    |peer, mut reader, kcp_listener| async move {
                        // create kcp peer
                        let (conv, kcp_peer) = loop {
                            if let Some(data) = reader.recv().await {
                                let data = data?;
                                if data.len() >= 24 {
                                    let mut read = data_rw::DataReader::from(&data);
                                    let conv: u32 = read.read_fixed()?;
                                    let peer = kcp_listener.create_kcp_peer(conv, &peer);
                                    peer.input(&data).await?;
                                    break (conv, peer);
                                } else if data.len() == 4 {
                                    kcp_listener.send_kcp_conv(&peer, &data).await?;
                                } else {
                                    peer.send(&data).await?;
                                }
                            } else {
                                log::error!("udp peer:{} channel is close", peer.get_addr());
                                return Ok(());
                            }
                        };

                        let peer = kcp_peer.clone();
                        tokio::spawn(async move {
                            kcp_listener.peers.write().await.insert(conv, peer.clone());
                            if let Err(err) = (kcp_listener.input)(peer).await {
                                log::error!("kcp input error:{}", err);
                            }
                            if let Some(peer) = kcp_listener.peers.write().await.remove(&conv) {
                                peer.close();
                            }
                        });

                        while let Some(Ok(data)) = reader.recv().await {
                            if let Err(err) = kcp_peer.input(&data).await {
                                log::error!("kcp peer input error:{err}");
                                break;
                            }
                        }

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

        Ok(kcp_listener)
    }

    /// 返回 kcp listener 智能指针
    pub fn builder(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// 开始服务
    pub async fn start(self: &Arc<Self>) -> KcpResult<()> {
        self.udp_server.start(self.clone()).await?;
        let listener = self.clone();
        tokio::task::spawn(async move {
            loop {
                let timestamp = Self::timestamp();
                for peer in listener.peers.read().await.values() {
                    if let Err(err) = peer.update(timestamp).await {
                        log::error!("update kcp peer:{} error:{}", peer.to_string(), err)
                    }
                }
            }
        });
        Ok(())
    }

    /// 首先判断 是否第一次发包
    /// 如果第一次发包 看看发的是不是 [u8;4] 是的话 生成一个conv id,给客户端发回
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
