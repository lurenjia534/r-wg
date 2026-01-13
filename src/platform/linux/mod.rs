//! Linux network configuration implementation.

mod network;

pub use network::{apply_network_config, cleanup_network_config, AppliedNetworkState, NetworkError};
