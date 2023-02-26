use std::error::Error as StdError;
use std::io::{self, ErrorKind};
use thiserror::Error;

/// KCP protocol errors
#[derive(Error, Debug)]
pub enum Error {
    #[error("conv inconsistent:{0}-{1}")]
    ConvInconsistent(u32, u32),
    #[error("invalid mtu:{0}")]
    InvalidMtu(usize),
    #[error("invalid segment size:{0}")]
    InvalidSegmentSize(usize),
    #[error("invalid segment data size:{0}-{1}")]
    InvalidSegmentDataSize(usize, usize),
    #[error("network io error:")]
    IoError(#[from] io::Error),
    #[error("need update")]
    NeedUpdate,
    #[error("recv queue empty")]
    RecvQueueEmpty,
    #[error("expecting fragment")]
    ExpectingFragment,
    #[error("unsupported cmd:{0}")]
    UnsupportedCmd(u8),
    #[error("user buf too big")]
    UserBufTooBig,
    #[error("user buf too small")]
    UserBufTooSmall(usize),
    #[error("udp pipe is broken")]
    BrokenPipe,
    #[error("other error:{0}")]
    Other(String),
}

fn make_io_error<T>(kind: ErrorKind, msg: T) -> io::Error
where
    T: Into<Box<dyn StdError + Send + Sync>>,
{
    io::Error::new(kind, msg)
}

impl From<Error> for io::Error {
    fn from(err: Error) -> io::Error {
        let kind = match err {
            Error::ConvInconsistent(..) => ErrorKind::Other,
            Error::InvalidMtu(..) => ErrorKind::Other,
            Error::InvalidSegmentSize(..) => ErrorKind::Other,
            Error::InvalidSegmentDataSize(..) => ErrorKind::Other,
            Error::IoError(err) => return err,
            Error::NeedUpdate => ErrorKind::Other,
            Error::RecvQueueEmpty => ErrorKind::WouldBlock,
            Error::ExpectingFragment => ErrorKind::WouldBlock,
            Error::UnsupportedCmd(..) => ErrorKind::Other,
            Error::UserBufTooBig => ErrorKind::Other,
            Error::UserBufTooSmall(..) => ErrorKind::Other,
            Error::BrokenPipe => ErrorKind::BrokenPipe,
            Error::Other(..) => ErrorKind::Other,
        };

        make_io_error(kind, err)
    }
}

/// KCP result
pub type KcpResult<T> = Result<T, Error>;
