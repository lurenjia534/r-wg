use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Language {
    English,
    ChineseSimplified,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LanguagePreference {
    #[default]
    System,
    English,
    ChineseSimplified,
}

impl LanguagePreference {
    pub(crate) fn resolve(self) -> Language {
        match self {
            Self::System => system_language(),
            Self::English => Language::English,
            Self::ChineseSimplified => Language::ChineseSimplified,
        }
    }
}

pub(crate) fn system_language() -> Language {
    let locale = sys_locale::get_locale()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if locale.starts_with("zh") {
        Language::ChineseSimplified
    } else {
        Language::English
    }
}

pub(crate) fn tr(language: Language, key: &'static str) -> &'static str {
    match language {
        Language::English => key,
        Language::ChineseSimplified => zh_cn(key),
    }
}

fn zh_cn(key: &'static str) -> &'static str {
    match key {
        "About" => "关于",
        "Activity" => "活动",
        "Ads/trackers blocked" => "阻止广告/跟踪器",
        "Ads/trackers/adult blocked" => "阻止广告/跟踪器/成人内容",
        "All profiles" => "所有配置",
        "Allowed IPs" => "允许的 IP",
        "Appearance" => "外观",
        "Appearance Policy" => "外观策略",
        "Appearance and remembered app defaults." => "外观与已记住的应用默认值。",
        "Auto" => "自动",
        "Auto Fill Missing Families" => "自动补齐缺失地址族",
        "Auto Follow Logs" => "自动跟随日志",
        "Auto Follow (Lock Selection)" => "自动跟随（锁定选择）",
        "Block traffic outside the tunnel during protected sessions" => {
            "受保护会话期间阻止隧道外流量"
        }
        "Choose the default range for charts and summaries." => "选择图表和摘要的默认时间范围。",
        "Choose the default right-side panel in Configs and the app language." => {
            "选择配置页默认右侧面板和应用语言。"
        }
        "Choose whether config DNS, system DNS, or presets take precedence." => {
            "选择配置 DNS、系统 DNS 或预设 DNS 的优先级。"
        }
        "Choose whether the app follows the OS, or stays pinned to light or dark." => {
            "选择应用跟随系统，或固定为亮色/暗色。"
        }
        "Choose which right-side panel opens first in Configs." => {
            "选择配置页默认打开的右侧面板。"
        }
        "Choose how DNS values are derived." => "选择 DNS 值的来源方式。",
        "Clear" => "清除",
        "Cloudflare (1.1.1.1)" => "Cloudflare (1.1.1.1)",
        "Coming Soon" => "即将推出",
        "CONTROL ROOM" => "控制台",
        "Configs" => "配置",
        "Copy" => "复制",
        "Copy All" => "全部复制",
        "Copy value" => "复制值",
        "Connection Security" => "连接安全",
        "Control how the runtime log viewer behaves." => "控制运行时日志查看器的行为。",
        "Core" => "核心",
        "configs" => "项配置",
        "Current throughput and accumulated transfer" => "当前吞吐量和累计传输",
        "DAITA Resources" => "DAITA 资源",
        "DAITA overhead" => "DAITA 开销",
        "DNS" => "DNS",
        "DNS Mode" => "DNS 模式",
        "DNS Preset" => "DNS 预设",
        "DNS settings will appear here." => "DNS 设置会显示在这里。",
        "DOCUMENT" => "文档",
        "Dark" => "暗色",
        "Dark Palette" => "暗色调色板",
        "Default" => "默认",
        "Delete" => "删除",
        "Defaults used when tunnel configs do not fully define DNS behavior." => {
            "隧道配置未完整定义 DNS 行为时使用的默认值。"
        }
        "Diagnostics" => "诊断",
        "Dirty" => "未保存",
        "Download" => "下载",
        "Download Speed" => "下载速度",
        "Draft" => "草稿",
        "Edit, validate, and manage tunnel profiles." => "编辑、验证和管理隧道配置。",
        "Enabled by default. Full-tunnel sessions keep extra platform guardrails active to reduce leak risk." => {
            "默认启用。全隧道会话会保留额外的平台防护，降低泄漏风险。"
        }
        "Endpoint" => "端点",
        "English" => "English",
        "Export" => "导出",
        "Families - Malware" => "家庭版 - 恶意软件",
        "Families - Malware + Adult" => "家庭版 - 恶意软件 + 成人内容",
        "Follow System" => "跟随系统",
        "Follow Config" => "跟随配置",
        "General" => "通用",
        "Handshake" => "握手",
        "Idle" => "空闲",
        "Ignore config DNS and force selected provider" => {
            "忽略配置 DNS，并强制使用选中的提供方"
        }
        "Import" => "导入",
        "Inspector View" => "检查器视图",
        "Keep DNS handling predictable across imported configs." => {
            "让导入配置的 DNS 处理保持可预测。"
        }
        "Keep the log pane pinned to the latest runtime events." => {
            "让日志面板始终停留在最新运行事件。"
        }
        "Collect local log lines and sync backend logs when the Logs page is open." => {
            "收集本地日志，并在打开日志页时同步后端日志。"
        }
        "Enable Log Viewer" => "启用日志查看器",
        "Language" => "语言",
        "Last Month" => "上月",
        "Last updated" => "最后更新",
        "Light" => "亮色",
        "Light Palette" => "亮色调色板",
        "Live" => "运行中",
        "Live session metrics and transport state" => "实时会话指标和传输状态",
        "Local IP" => "本地 IP",
        "Logs" => "日志",
        "Logs cleared" => "日志已清除",
        "Logs copied" => "日志已复制",
        "Log viewer disabled in Preferences." => "日志查看器已在偏好设置中关闭。",
        "LIBRARY" => "配置库",
        "Manage appearance, defaults, and system integration in one place." => {
            "在一个位置管理外观、默认值和系统集成。"
        }
        "Manage the helper service required for routes, DNS changes, and tunnel startup." => {
            "管理路由、DNS 更改和隧道启动所需的辅助服务。"
        }
        "Malware Blocking" => "恶意软件拦截",
        "Malware + Adult" => "恶意软件 + 成人内容",
        "Memory" => "内存",
        "Monitoring" => "监控",
        "Network" => "网络",
        "Needs restart" => "需要重启",
        "New" => "新建",
        "No configs yet. Import a file or start a new draft." => {
            "还没有配置。导入文件或新建草稿。"
        }
        "No filtering" => "不过滤",
        "No selection" => "未选择",
        "No tunnel" => "无隧道",
        "Only fill missing IPv4/IPv6 DNS families" => {
            "只补齐缺失的 IPv4/IPv6 DNS 地址族"
        }
        "Open preferences" => "打开偏好设置",
        "Overview" => "概览",
        "Override All" => "全部覆盖",
        "Paste" => "粘贴",
        "Peers" => "对端",
        "Preferences" => "偏好设置",
        "Preferred Traffic Range" => "首选流量范围",
        "Preview" => "预览",
        "Privileged Backend" => "特权后端",
        "Protected" => "已保护",
        "Proxies" => "代理",
        "Quantum protected" => "量子安全保护",
        "Ready" => "就绪",
        "Remembered monitoring behavior and chart defaults." => "已记住的监控行为和图表默认值。",
        "Remembered" => "已记住",
        "Rename" => "重命名",
        "Require local approval and control upcoming tunnel hardening behavior." => {
            "要求本地确认，并控制后续隧道加固行为。"
        }
        "Route Table" => "路由表",
        "Route Map" => "路由图",
        "Rules" => "规则",
        "Running" => "运行中",
        "Running Tunnel" => "运行中的隧道",
        "Runtime Health" => "运行健康",
        "Runtime health, selected config reference, and traffic posture in one surface." => {
            "在一个界面查看运行健康、选中配置参考和流量状态。"
        }
        "Save" => "保存",
        "Save & Restart" => "保存并重启",
        "Save as new" => "另存为新配置",
        "Saved" => "已保存",
        "Saved config values shown as a stable reference" => {
            "以稳定参考方式显示已保存配置值"
        }
        "Saved source" => "已保存来源",
        "SETTINGS" => "设置",
        "Service Status" => "服务状态",
        "Set the UI language. System follows your OS locale when it is available." => {
            "设置界面语言。系统选项会在可用时跟随操作系统区域设置。"
        }
        "Selected" => "已选择",
        "Selected Config Preview" => "选中配置预览",
        "Selected preview" => "选中预览",
        "Settings" => "设置",
        "Simplified Chinese" => "简体中文",
        "Soon" => "即将",
        "Source" => "来源",
        "Standard" => "标准",
        "Switch to dark mode" => "切换到暗色模式",
        "Switch to light mode" => "切换到亮色模式",
        "System" => "系统",
        "System dns" => "系统 DNS",
        "System DNS" => "系统 DNS",
        "This Month" => "本月",
        "This section is under construction." => "此区域仍在建设中。",
        "Theme Files" => "主题文件",
        "Today" => "今天",
        "Tools" => "工具",
        "Traffic" => "流量",
        "Traffic Stats" => "流量统计",
        "Tunnel configs" => "隧道配置",
        "Tunnel idle" => "隧道空闲",
        "Troubleshooting" => "故障排查",
        "Tunnel" => "隧道",
        "TX Padding" => "TX 填充",
        "TX Decoy" => "TX 诱饵",
        "RX Padding" => "RX 填充",
        "RX Decoy" => "RX 诱饵",
        "Updated" => "已更新",
        "Updating" => "更新中",
        "Upload" => "上传",
        "Upload Speed" => "上传速度",
        "Uptime" => "运行时间",
        "Unfiltered" => "不过滤",
        "Unsaved draft in editor." => "编辑器中有未保存草稿。",
        "Use DNS only from the config file" => "只使用配置文件中的 DNS",
        "Use DNS from the system resolver" => "使用系统解析器的 DNS",
        "Use DNS settings from the config file." => "使用配置文件中的 DNS 设置。",
        "Use system default DNS." => "使用系统默认 DNS。",
        "WIREGUARD CONFIG" => "WIREGUARD 配置",
        "Workspace" => "工作区",
        "line" => "行",
        "lines" => "行",
        "Connected" => "已连接",
        "Connections" => "连接",
        "Providers" => "提供方",
        "Topology" => "拓扑",
        _ => key,
    }
}
