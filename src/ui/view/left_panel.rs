use gpui::*;

use gpui_component::{
    h_flex,
    sidebar::{Sidebar, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem},
    v_flex, ActiveTheme as _, Icon, IconName, Selectable as _,
};

use super::super::state::{SidebarItem, WgApp};
use super::data::ViewData;

/// 左侧导航栏：分组 + 图标 + 选中态，仅负责布局与高亮。
pub(crate) fn render_left_panel(
    app: &mut WgApp,
    _data: &ViewData,
    cx: &mut Context<WgApp>,
) -> impl IntoElement {
    let header = SidebarHeader::new()
        .child(
            div()
                .size_8()
                .rounded(cx.theme().radius)
                .bg(cx.theme().sidebar_primary)
                .text_color(cx.theme().sidebar_primary_foreground)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(IconName::LayoutDashboard).size_4()),
        )
        .child(
            v_flex()
                .gap_0()
                .text_sm()
                .line_height(relative(1.2))
                .child("r-wg")
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Dashboard"),
                ),
        );

    let footer = SidebarFooter::new()
        .justify_between()
        .child(
            h_flex()
                .gap_2()
                .child(Icon::new(IconName::Info).size_4())
                .child(div().text_sm().child("About")),
        )
        .child(Icon::new(IconName::Settings).size_4())
        .selected(app.ui_prefs.sidebar_active == SidebarItem::About)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event, _window, cx| {
                this.ui_prefs.sidebar_active = SidebarItem::About;
                cx.notify();
            }),
        );

    Sidebar::left()
        .collapsible(false)
        .w(px(230.))
        .header(header)
        .child(sidebar_group(
            "Overview",
            &[
                SidebarItem::Overview,
                SidebarItem::TrafficStats,
                SidebarItem::Connections,
                SidebarItem::Logs,
            ],
            app,
            cx,
        ))
        .child(sidebar_group(
            "Proxy",
            &[
                SidebarItem::Proxies,
                SidebarItem::Rules,
                SidebarItem::Dns,
                SidebarItem::Providers,
            ],
            app,
            cx,
        ))
        .child(sidebar_group(
            "Settings",
            &[SidebarItem::Configs, SidebarItem::Advanced],
            app,
            cx,
        ))
        .child(sidebar_group(
            "Labs",
            &[SidebarItem::Topology, SidebarItem::RouteMap],
            app,
            cx,
        ))
        .footer(footer)
}

fn sidebar_group(
    label: &'static str,
    items: &[SidebarItem],
    app: &WgApp,
    cx: &mut Context<WgApp>,
) -> SidebarGroup<SidebarMenu> {
    SidebarGroup::new(label).child(SidebarMenu::new().children(items.iter().map(|item| {
        let item = *item;
        SidebarMenuItem::new(item.label())
            .icon(Icon::new(item.icon()).size_4())
            .active(app.ui_prefs.sidebar_active == item)
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.ui_prefs.sidebar_active = item;
                cx.notify();
            }))
    })))
}
