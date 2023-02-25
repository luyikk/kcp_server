pub mod error;
mod kcp;
mod kcp_config;

/// The `KCP` prelude
pub mod prelude {
    pub use super::error::*;
    pub use super::kcp::{get_conv, set_conv, Kcp};
    pub use super::kcp_config::*;
}
