//! Linux network configuration implementation.

mod network;

pub use network::{
    apply_network_config, attempt_startup_repair, cleanup_network_config, AppliedNetworkState,
    NetworkError,
};
