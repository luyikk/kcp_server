mod kcp;

pub mod prelude {
    use crate::kcp;
    pub mod kcp_module {
        use crate::kcp;
        pub use kcp::kcp_module::prelude::{
            get_conv, set_conv, Error, KcpConfig, KcpNoDelayConfig, KcpResult,
        };
    }

    pub use kcp::kcp_server::kcp_listener::*;
    pub use kcp::kcp_server::kcp_peer::*;
}
