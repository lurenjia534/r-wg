use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 记录本次使用的 DNS 后端与其回滚信息，供 stop/cleanup 时撤销。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::platform::linux::network) struct DnsState {
    pub(super) backend: DnsBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum DnsBackend {
    /// systemd-resolved: 对接口设置 DNS，可通过 resolvectl revert 回滚。
    Resolved,
    /// resolvconf/openresolv: 写入接口条目，可通过 resolvconf -d 回滚。
    Resolvconf,
    /// NetworkManager: 保存连接原状态，失败或停止时恢复。
    NetworkManager { connections: Vec<NmConnectionState> },
    /// 直接写 /etc/resolv.conf（仅当是普通文件）。
    ResolvConf { path: PathBuf, original: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct NmConnectionState {
    pub(super) name: String,
    pub(super) device: String,
    pub(super) ipv4_dns: String,
    pub(super) ipv4_ignore_auto: String,
    pub(super) ipv4_search: String,
    pub(super) ipv4_priority: String,
    pub(super) ipv6_dns: String,
    pub(super) ipv6_ignore_auto: String,
    pub(super) ipv6_search: String,
    pub(super) ipv6_priority: String,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum DnsBackendKind {
    Resolved,
    Resolvconf,
    NetworkManager,
    ResolvConf,
}

impl DnsBackendKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Resolvconf => "resolvconf",
            Self::NetworkManager => "network-manager",
            Self::ResolvConf => "resolv.conf",
        }
    }
}

#[derive(Debug)]
pub(super) struct ResolvConfInfo {
    /// /etc/resolv.conf 路径（固定）。
    pub(super) path: PathBuf,
    /// 是否为符号链接，用于判断是系统管理还是手写文件。
    pub(super) is_symlink: bool,
    /// 若为符号链接，记录目标，帮助识别后端类型。
    pub(super) target: Option<PathBuf>,
    /// 读取内容用于启发式判断（比如 systemd-resolved/NetworkManager 标记）。
    pub(super) contents: Option<String>,
}
