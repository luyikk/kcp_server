mod error;
mod kcp;
pub mod kcp_config;


/// The `KCP` prelude
pub mod prelude {
    pub use super::{get_conv, Kcp};
}

pub use error::Error;
pub use kcp::{get_conv, set_conv, Kcp};

/// KCP result
pub type KcpResult<T> = Result<T, Error>;
