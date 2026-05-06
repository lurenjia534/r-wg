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
        "Appearance" => "外观",
        "Appearance Policy" => "外观策略",
        "Appearance and remembered app defaults." => "外观与已记住的应用默认值。",
        "Auto" => "自动",
        "Auto Follow Logs" => "自动跟随日志",
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
        "Coming Soon" => "即将推出",
        "Configs" => "配置",
        "Connection Security" => "连接安全",
        "Control how the runtime log viewer behaves." => "控制运行时日志查看器的行为。",
        "Core" => "核心",
        "DAITA Resources" => "DAITA 资源",
        "DNS" => "DNS",
        "DNS Mode" => "DNS 模式",
        "DNS Preset" => "DNS 预设",
        "Dark" => "暗色",
        "Dark Palette" => "暗色调色板",
        "Defaults used when tunnel configs do not fully define DNS behavior." => {
            "隧道配置未完整定义 DNS 行为时使用的默认值。"
        }
        "Diagnostics" => "诊断",
        "Dirty" => "未保存",
        "Enabled by default. Full-tunnel sessions keep extra platform guardrails active to reduce leak risk." => {
            "默认启用。全隧道会话会保留额外的平台防护，降低泄漏风险。"
        }
        "English" => "English",
        "Follow System" => "跟随系统",
        "General" => "通用",
        "Idle" => "空闲",
        "Inspector View" => "检查器视图",
        "Keep DNS handling predictable across imported configs." => {
            "让导入配置的 DNS 处理保持可预测。"
        }
        "Keep the log pane pinned to the latest runtime events." => {
            "让日志面板始终停留在最新运行事件。"
        }
        "Language" => "语言",
        "Last Month" => "上月",
        "Light" => "亮色",
        "Light Palette" => "亮色调色板",
        "Live" => "运行中",
        "Logs" => "日志",
        "Manage appearance, defaults, and system integration in one place." => {
            "在一个位置管理外观、默认值和系统集成。"
        }
        "Manage the helper service required for routes, DNS changes, and tunnel startup." => {
            "管理路由、DNS 更改和隧道启动所需的辅助服务。"
        }
        "Monitoring" => "监控",
        "Network" => "网络",
        "No tunnel" => "无隧道",
        "Open preferences" => "打开偏好设置",
        "Overview" => "概览",
        "Preferences" => "偏好设置",
        "Preferred Traffic Range" => "首选流量范围",
        "Preview" => "预览",
        "Privileged Backend" => "特权后端",
        "Protected" => "已保护",
        "Proxies" => "代理",
        "Ready" => "就绪",
        "Remembered monitoring behavior and chart defaults." => "已记住的监控行为和图表默认值。",
        "Require local approval and control upcoming tunnel hardening behavior." => {
            "要求本地确认，并控制后续隧道加固行为。"
        }
        "Route Map" => "路由图",
        "Rules" => "规则",
        "SETTINGS" => "设置",
        "Service Status" => "服务状态",
        "Set the UI language. System follows your OS locale when it is available." => {
            "设置界面语言。系统选项会在可用时跟随操作系统区域设置。"
        }
        "Settings" => "设置",
        "Simplified Chinese" => "简体中文",
        "Soon" => "即将",
        "Switch to dark mode" => "切换到暗色模式",
        "Switch to light mode" => "切换到亮色模式",
        "System" => "系统",
        "This Month" => "本月",
        "Theme Files" => "主题文件",
        "Today" => "今天",
        "Tools" => "工具",
        "Traffic" => "流量",
        "Traffic Stats" => "流量统计",
        "Troubleshooting" => "故障排查",
        "Tunnel" => "隧道",
        "Updating" => "更新中",
        "Workspace" => "工作区",
        "Connected" => "已连接",
        "Connections" => "连接",
        "Providers" => "提供方",
        "Topology" => "拓扑",
        _ => key,
    }
}
