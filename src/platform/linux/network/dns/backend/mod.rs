mod network_manager;
mod resolv_conf;
mod resolvconf;
mod resolved;

pub(super) use self::network_manager::{apply_network_manager, cleanup_network_manager};
pub(super) use self::resolv_conf::{apply_resolv_conf_file, cleanup_resolv_conf_file};
pub(super) use self::resolvconf::{apply_resolvconf, cleanup_resolvconf};
pub(super) use self::resolved::{apply_resolved, cleanup_resolved};

#[cfg(test)]
pub(super) use self::resolv_conf::build_resolv_conf_contents;
