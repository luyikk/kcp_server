use std::sync::{Weak, Arc};
use crate::kcp::KcpPeer;
use crate::buffer_pool::BuffPool;
use bytes::{Bytes, Buf, BufMut};
use std::error::Error;
use log::*;
use std::net::SocketAddr;
use xbinary::*;

use tokio::time::{delay_for, Duration};
use std::cell::RefCell;

/// 玩家PEER
pub struct ClientPeer{
    pub session_id:u32,
    pub kcp_peer:Weak<KcpPeer<Arc<ClientPeer>>>,
    pub buff_pool:RefCell<BuffPool>,
    pub is_open_zero:bool
}

unsafe  impl Send for ClientPeer{}
unsafe  impl Sync for ClientPeer{}


impl Drop for ClientPeer{
    fn drop(&mut self) {
        info!("client_peer:{} is drop",self.session_id);
    }
}

impl ClientPeer{
    /// 创建一个玩家PEER
    pub fn new(session_id:u32, kcp_peer:Weak<KcpPeer<Arc<ClientPeer>>>)->ClientPeer{
        ClientPeer{
            session_id,
            kcp_peer,
            buff_pool:RefCell::new(BuffPool::new(512*1024)),
            is_open_zero:false
        }
    }

    /// 获取IP地址+端口号
    pub fn get_addr(&self)->Option<SocketAddr>{
        self.kcp_peer.upgrade().map(|x|{
            x.addr
        })
    }

    /// OPEN 服务器
    pub fn open(&self,_server_id:u32){

    }

    /// 网络数据包输入,处理
    pub async fn input_buff(&self,buff:&[u8])->Result<(),Box<dyn Error>>{
       let input_data_array= {
            let mut input_data_vec = Vec::with_capacity(1);
            let mut buff_pool = self.buff_pool.borrow_mut();
            buff_pool.write(buff);
            loop {
                match buff_pool.read() {
                    Ok(data) => {
                        if let Some(data) = data {
                            input_data_vec.push(Bytes::from(data));
                        } else {
                            buff_pool.advance();
                            break;
                        }
                    },
                    Err(msg) => {
                        error!("{}-{:?} error:{}", self.session_id, self.get_addr(), msg);
                        buff_pool.reset();
                        break;
                    }
                }
            }
           input_data_vec
        };

        for data in input_data_array {
            self.input_data(data).await?;
        }

        Ok(())
    }

    /// 数据包处理
    async fn input_data(&self,data:Bytes)->Result<(),Box<dyn Error>>{
        let mut reader=XBRead::new(data);
        let server_id= reader.get_u32_le();
        match server_id {
            0xFFFFFFFF=>{
                self.send(server_id,&reader).await?;
            },
            0=>{
                if !self.is_open_zero{
                    self.kick().await?;
                    info!("Peer:{}-{:?} not open read data Disconnect it",self.session_id,self.get_addr());
                    return Ok(())
                }


            },
            0xEEEEEEEE=>{
                if !self.is_open_zero{
                    self.kick().await?;
                    info!("Peer:{}-{:?} not open read data Disconnect it",self.session_id,self.get_addr());
                    return Ok(())
                }

            },
            _=>{
                if !self.is_open_zero{
                    self.kick().await?;
                    info!("Peer:{}-{:?} not open read data Disconnect it",self.session_id,self.get_addr());
                    return Ok(())
                }

            }
        }

        Ok(())
    }


    /// 先发送断线包等待多少毫秒清理内存
    pub async fn kick_wait_ms(&self,ms:u32)->Result<(),Box<dyn Error>>{
        if ms==3111{
            self.disconnect_now();
        }
        else{
            self.send_close(0).await?;
            delay_for(Duration::from_millis(ms as u64)).await;
            self.disconnect_now();
        }
        Ok(())
    }

    /// 发送 CLOSE 0 后立即断线清理内存
    async fn kick(&self)->Result<(),Box<dyn Error>>{
        self.send_close(0).await?;
        self.disconnect_now();
        Ok(())
    }

    /// 立即断线,清理内存
    pub fn disconnect_now(&self){
        if let Some(kcp_peer)= self.kcp_peer.upgrade() {
            kcp_peer.disconnect();
            info!("disconnect peer:{}",self.session_id);
        }
    }

    /// 发送数据
    pub async fn send(&self,session_id:u32,data:&[u8])->Result<usize,Box<dyn Error>>{
        if let Some(kcp_peer)= self.kcp_peer.upgrade() {
            let mut writer=XBWrite::new();
            writer.put_u32_le(0);
            writer.put_u32_le(session_id);
            writer.write(data);
            writer.set_position(0);
            writer.put_u32_le(writer.len() as u32 -4);
            return Ok(kcp_peer.send(&writer)?)
        }
        Ok(0)
    }

    /// 发送CLOSE 0
    pub async fn send_close(&self,service_id:u32)->Result<usize,Box<dyn Error>>{
        if let Some(kcp_peer)= self.kcp_peer.upgrade() {
            let mut writer=XBWrite::new();
            writer.put_u32_le(0);
            writer.put_u32_le(0xFFFFFFFF);
            writer.write_string_bit7_len("close");
            writer.bit7_write_u32(service_id);
            return Ok(kcp_peer.send(&writer)?)
        }
        Ok(0)
    }

}