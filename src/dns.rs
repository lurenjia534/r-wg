use std::net::IpAddr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// DNS 模式（全局 UI 状态）。
///
/// 这些模式只影响启动时“解析后的配置”如何处理 DNS，不会在运行中自动生效。
/// Windows helper 模式下，它也会随启动请求一起跨 IPC 发送到管理员 helper。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DnsMode {
    /// 完全遵循配置文件中的 DNS。
    FollowConfig,
    /// 强制使用系统默认 DNS（即清空配置 DNS）。
    UseSystemDns,
    /// 只补齐缺失的 IPv4/IPv6 DNS，已有配置保持不变。
    AutoFillMissingFamilies,
    /// 忽略配置 DNS，强制使用预设 DNS。
    OverrideAll,
}

impl DnsMode {
    /// UI 展示用标签。
    pub fn label(self) -> &'static str {
        match self {
            Self::FollowConfig => "Follow Config",
            Self::UseSystemDns => "System dns",
            Self::AutoFillMissingFamilies => "Auto Fill Missing Families",
            Self::OverrideAll => "Override All",
        }
    }
}

/// DNS 预设提供商（UI/后端共用）。
/// 预设值会进入启动请求，确保 UI 与 helper 对同一套地址表达成一致。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DnsPreset {
    CloudflareStandard,
    CloudflareMalware,
    CloudflareMalwareAdult,
    AdguardDefault,
    AdguardUnfiltered,
    AdguardFamily,
}

/// 预设的展示信息与地址列表（静态表）。
pub struct DnsPresetInfo {
    pub title: &'static str,
    pub note: &'static str,
    pub ipv4: &'static [&'static str],
    pub ipv6: &'static [&'static str],
}

impl DnsPreset {
    /// 获取用于 UI 的标题/说明/地址列表。
    pub fn info(self) -> DnsPresetInfo {
        match self {
            Self::CloudflareStandard => DnsPresetInfo {
                title: "Standard",
                note: "No filtering",
                ipv4: &["1.1.1.1", "1.0.0.1"],
                ipv6: &["2606:4700:4700::1111", "2606:4700:4700::1001"],
            },
            Self::CloudflareMalware => DnsPresetInfo {
                title: "Malware Blocking",
                note: "Families - Malware",
                ipv4: &["1.1.1.2", "1.0.0.2"],
                ipv6: &["2606:4700:4700::1112", "2606:4700:4700::1002"],
            },
            Self::CloudflareMalwareAdult => DnsPresetInfo {
                title: "Malware + Adult",
                note: "Families - Malware + Adult",
                ipv4: &["1.1.1.3", "1.0.0.3"],
                ipv6: &["2606:4700:4700::1113", "2606:4700:4700::1003"],
            },
            Self::AdguardDefault => DnsPresetInfo {
                title: "Default",
                note: "Ads/trackers blocked",
                ipv4: &["94.140.14.14", "94.140.15.15"],
                ipv6: &["2a10:50c0::ad1:ff", "2a10:50c0::ad2:ff"],
            },
            Self::AdguardUnfiltered => DnsPresetInfo {
                title: "Unfiltered",
                note: "No filtering",
                ipv4: &["94.140.14.140", "94.140.14.141"],
                ipv6: &["2a10:50c0::1:ff", "2a10:50c0::2:ff"],
            },
            Self::AdguardFamily => DnsPresetInfo {
                title: "Family",
                note: "Ads/trackers/adult blocked",
                ipv4: &["94.140.14.15", "94.140.15.16"],
                ipv6: &["2a10:50c0::bad1:ff", "2a10:50c0::bad2:ff"],
            },
        }
    }

    /// 仅 IPv4 地址（转换为 IpAddr）。
    pub fn ipv4_addrs(self) -> Vec<IpAddr> {
        parse_ip_list(self.info().ipv4)
    }

    /// 仅 IPv6 地址（转换为 IpAddr）。
    pub fn ipv6_addrs(self) -> Vec<IpAddr> {
        parse_ip_list(self.info().ipv6)
    }

    /// 合并 IPv4 + IPv6 地址（保持原有顺序）。
    pub fn all_addrs(self) -> Vec<IpAddr> {
        let info = self.info();
        let mut out = Vec::with_capacity(info.ipv4.len() + info.ipv6.len());
        out.extend(parse_ip_list(info.ipv4));
        out.extend(parse_ip_list(info.ipv6));
        out
    }
}

/// 启动时使用的 DNS 选择项（来自 UI 全局设置）。
/// 这是 UI 侧 DNS 配置传给后端的统一载体，也是 Windows helper IPC 的一部分。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct DnsSelection {
    pub mode: DnsMode,
    pub preset: DnsPreset,
}

impl DnsSelection {
    /// 构造器，便于从 UI 状态创建。
    pub fn new(mode: DnsMode, preset: DnsPreset) -> Self {
        Self { mode, preset }
    }
}

/// 按选择项改写配置中的 DNS。
///
/// 规则：
/// - FollowConfig：不做任何修改。
/// - UseSystemDns：清空 servers/search，让后端不写入 DNS。
/// - AutoFillMissingFamilies：仅补缺少的 IPv4/IPv6（保留既有 servers 与 search）。
/// - OverrideAll：用预设替换 servers，并清空 search。
pub fn apply_dns_selection(
    servers: &mut Vec<IpAddr>,
    search: &mut Vec<String>,
    selection: DnsSelection,
) {
    match selection.mode {
        // 原样使用配置。
        DnsMode::FollowConfig => {}
        // 清空配置中的 DNS，意味着系统默认 DNS 生效。
        DnsMode::UseSystemDns => {
            servers.clear();
            search.clear();
        }
        // 只补齐缺失的 IPv4/IPv6，不覆盖已有配置。
        DnsMode::AutoFillMissingFamilies => {
            let has_v4 = servers.iter().any(|ip| ip.is_ipv4());
            let has_v6 = servers.iter().any(|ip| ip.is_ipv6());
            if !has_v4 {
                servers.extend(selection.preset.ipv4_addrs());
            }
            if !has_v6 {
                servers.extend(selection.preset.ipv6_addrs());
            }
        }
        // 强制使用预设，并清空 search（完全忽略配置 DNS）。
        DnsMode::OverrideAll => {
            *servers = selection.preset.all_addrs();
            search.clear();
        }
    }
}

/// 把静态字符串列表解析为 IP 地址。
///
/// 预设表是硬编码数据，解析失败意味着代码错误，因此使用 expect。
fn parse_ip_list(list: &'static [&'static str]) -> Vec<IpAddr> {
    list.iter()
        .map(|addr| IpAddr::from_str(addr).expect("preset DNS must be valid"))
        .collect()
}
