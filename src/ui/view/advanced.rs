use std::env::consts::{ARCH, OS};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use gpui::{
    div, prelude::FluentBuilder as _, px, Axis, Context, Div, Entity, Hsla, IntoElement,
    ParentElement, SharedString, Styled, Timer, Window,
};
use gpui_component::theme::{Colorize as _, Theme, ThemeMode};
use gpui_component::{
    button::{Button, ButtonGroup, ButtonVariant, ButtonVariants},
    description_list::DescriptionList,
    dialog::DialogButtonProps,
    group_box::GroupBoxVariant,
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    v_flex, ActiveTheme as _, Disableable as _, Selectable, Sizable as _, StyledExt as _,
    WindowExt,
};
use r_wg::backend::wg::PrivilegedServiceAction;
use r_wg::dns::{DnsMode, DnsPreset};

use super::super::persistence;
use super::super::state::{
    BackendDiagnostic, BackendHealth, ConfigInspectorTab, SidebarItem, TrafficPeriod, WgApp,
};
use super::super::theme_lint::{self, ThemeLintItem, ThemeLintSeverity};
use super::super::themes::{self, AppearancePolicy};
use super::widgets::{backend_status_tag, PageShell, PageShellHeader};

// API-preserving split: page assembly, theme preview, preferences controls,
// system/backend tooling, and tests now live in focused files.
include!("advanced/page.rs");
include!("advanced/theme.rs");
include!("advanced/preferences.rs");
include!("advanced/system.rs");
include!("advanced/tests.rs");
