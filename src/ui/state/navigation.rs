use gpui_component::IconName;
use serde::{Deserialize, Serialize};

// Shared UI enums for inspector, navigation, and route-map filters.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProxiesViewMode {
    List,
    Gallery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteMapMode {
    Flow,
    Routes,
    Explain,
    Events,
}

impl RouteMapMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Flow => "Flow",
            Self::Routes => "Routes",
            Self::Explain => "Explain",
            Self::Events => "Events",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteFamilyFilter {
    All,
    Ipv4,
    Ipv6,
}

impl RouteFamilyFilter {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Ipv4 => "IPv4",
            Self::Ipv6 => "IPv6",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProxyRunningFilter {
    All,
    Running,
    Idle,
}

/// 左侧导航栏的选中项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarItem {
    Overview,
    TrafficStats,
    Connections,
    Logs,
    Proxies,
    Rules,
    Dns,
    Providers,
    Configs,
    Advanced,
    Topology,
    RouteMap,
    Tools,
    About,
}

impl SidebarItem {
    pub(crate) fn nav_key(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::TrafficStats => "traffic-stats",
            Self::Connections => "connections",
            Self::Logs => "logs",
            Self::Proxies => "proxies",
            Self::Rules => "rules",
            Self::Dns => "dns",
            Self::Providers => "providers",
            Self::Configs => "configs",
            Self::Advanced => "preferences",
            Self::Topology => "topology",
            Self::RouteMap => "route-map",
            Self::Tools => "tools",
            Self::About => "about",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::TrafficStats => "Traffic Stats",
            Self::Connections => "Connections",
            Self::Logs => "Logs",
            Self::Proxies => "Proxies",
            Self::Rules => "Rules",
            Self::Dns => "DNS",
            Self::Providers => "Providers",
            Self::Configs => "Configs",
            Self::Advanced => "Preferences",
            Self::Topology => "Topology",
            Self::RouteMap => "Route Map",
            Self::Tools => "Tools",
            Self::About => "About",
        }
    }

    pub(crate) fn icon(self) -> IconName {
        match self {
            Self::Overview => IconName::LayoutDashboard,
            Self::TrafficStats => IconName::ChartPie,
            Self::Connections => IconName::Replace,
            Self::Logs => IconName::SquareTerminal,
            Self::Proxies => IconName::Globe,
            Self::Rules => IconName::Inspector,
            Self::Dns => IconName::Search,
            Self::Providers => IconName::Building2,
            Self::Configs => IconName::File,
            Self::Advanced => IconName::Settings2,
            Self::Topology => IconName::Frame,
            Self::RouteMap => IconName::Map,
            Self::Tools => IconName::Search,
            Self::About => IconName::Info,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingDraftAction {
    SelectConfig(u64),
    ActivateSidebar(SidebarItem),
    NewDraft,
    Import,
    Paste,
    DeleteCurrent,
    RestartTunnel,
}
