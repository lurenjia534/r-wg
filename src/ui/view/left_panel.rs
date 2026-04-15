use gpui::prelude::FluentBuilder as _;
use gpui::*;
use std::sync::Arc;

use gpui_component::{
    h_flex,
    sidebar::{Sidebar, SidebarGroup, SidebarToggleButton},
    tooltip::Tooltip,
    v_flex, ActiveTheme as _, Collapsible, Icon, IconName, StyledExt as _,
};

use super::super::state::{SidebarItem, WgApp};

const SIDEBAR_EXPANDED_WIDTH: f32 = 256.0;
const SIDEBAR_FORCE_COLLAPSE_BREAKPOINT: f32 = 1160.0;
const SIDEBAR_OVERLAY_BREAKPOINT: f32 = 820.0;
const SIDEBAR_OVERLAY_WIDTH: f32 = 312.0;

#[derive(Clone, Copy)]
enum NavSuffixTone {
    Muted,
    Accent,
}

#[derive(Clone)]
enum NavSuffix {
    Label(&'static str, NavSuffixTone),
}

#[derive(Clone)]
struct NavEntry {
    key: &'static str,
    item: Option<SidebarItem>,
    label: &'static str,
    icon: IconName,
    active: bool,
    disabled: bool,
    default_open: bool,
    suffix: Option<NavSuffix>,
    children: Vec<Self>,
}

impl NavEntry {
    fn item(item: SidebarItem, active: bool) -> Self {
        Self {
            key: item.nav_key(),
            item: Some(item),
            label: item.label(),
            icon: item.icon(),
            active,
            disabled: false,
            default_open: false,
            suffix: None,
            children: Vec::new(),
        }
    }

    fn group(
        key: &'static str,
        label: &'static str,
        icon: IconName,
        children: Vec<Self>,
        default_open: bool,
    ) -> Self {
        let active = children.iter().any(|child| child.active);
        Self {
            key,
            item: None,
            label,
            icon,
            active,
            disabled: false,
            default_open,
            suffix: None,
            children,
        }
    }

    fn suffix(mut self, label: &'static str, tone: NavSuffixTone) -> Self {
        self.suffix = Some(NavSuffix::Label(label, tone));
        self
    }

    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Clone, PartialEq, Eq)]
struct LeftPanelCacheKey {
    active: SidebarItem,
    sidebar_collapsed: bool,
    runtime_running: bool,
    runtime_busy: bool,
    quantum_protected: bool,
    running_name: Option<String>,
    draft_dirty: bool,
}

#[derive(Clone)]
struct LeftPanelSnapshot {
    sidebar_collapsed: bool,
    runtime_running: bool,
    quantum_protected: bool,
    status_text: SharedString,
    primary: Vec<NavEntry>,
    network: Vec<NavEntry>,
    footer: Vec<NavEntry>,
}

#[derive(Clone, Copy)]
enum LeftPanelPresentation {
    Docked,
    Overlay,
}

pub(crate) struct LeftPanelState {
    app: Entity<WgApp>,
    cache_key: Option<LeftPanelCacheKey>,
    snapshot: Option<Arc<LeftPanelSnapshot>>,
}

impl LeftPanelState {
    fn new(app: Entity<WgApp>, _cx: &mut Context<Self>) -> Self {
        Self {
            app,
            cache_key: None,
            snapshot: None,
        }
    }

    fn apply_snapshot(
        &mut self,
        next_key: LeftPanelCacheKey,
        next_snapshot: LeftPanelSnapshot,
        cx: &mut Context<Self>,
    ) {
        if self.cache_key.as_ref() == Some(&next_key) {
            return;
        }
        self.cache_key = Some(next_key);
        self.snapshot = Some(Arc::new(next_snapshot));
        cx.notify();
    }
}

#[derive(IntoElement)]
struct NavMenu {
    app: Entity<WgApp>,
    collapsed: bool,
    items: Vec<NavEntry>,
}

impl NavMenu {
    fn new(app: Entity<WgApp>, items: Vec<NavEntry>) -> Self {
        Self {
            app,
            collapsed: false,
            items,
        }
    }
}

#[derive(IntoElement)]
struct LeftPanelSurface {
    app: Entity<WgApp>,
    snapshot: Arc<LeftPanelSnapshot>,
    presentation: LeftPanelPresentation,
}

impl LeftPanelSurface {
    fn new(
        app: Entity<WgApp>,
        snapshot: Arc<LeftPanelSnapshot>,
        presentation: LeftPanelPresentation,
    ) -> Self {
        Self {
            app,
            snapshot,
            presentation,
        }
    }
}

impl Collapsible for NavMenu {
    fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    fn is_collapsed(&self) -> bool {
        self.collapsed
    }
}

impl RenderOnce for NavMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex().gap_1().children(
            self.items
                .into_iter()
                .map(|entry| render_nav_entry(entry, self.collapsed, self.app.clone(), window, cx)),
        )
    }
}

impl Render for LeftPanelState {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        LeftPanelSurface::new(
            self.app.clone(),
            self.snapshot
                .as_ref()
                .expect("left panel snapshot should exist")
                .clone(),
            LeftPanelPresentation::Docked,
        )
    }
}

impl RenderOnce for LeftPanelSurface {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let collapsed = match self.presentation {
            LeftPanelPresentation::Docked => {
                sidebar_is_collapsed(self.snapshot.sidebar_collapsed, window)
            }
            LeftPanelPresentation::Overlay => false,
        };
        let header = render_sidebar_header(
            &self.snapshot,
            self.app.clone(),
            collapsed,
            self.presentation,
            cx,
        );
        let footer =
            NavMenu::new(self.app.clone(), self.snapshot.footer.clone()).collapsed(collapsed);

        Sidebar::left()
            .collapsible(matches!(self.presentation, LeftPanelPresentation::Docked))
            .collapsed(collapsed)
            .w(px(SIDEBAR_EXPANDED_WIDTH))
            .header(header)
            .when(
                matches!(self.presentation, LeftPanelPresentation::Overlay),
                |this| this.w_full().border_r_0(),
            )
            .child(SidebarGroup::new("Core").child(NavMenu::new(
                self.app.clone(),
                self.snapshot.primary.clone(),
            )))
            .child(SidebarGroup::new("Network").child(NavMenu::new(
                self.app.clone(),
                self.snapshot.network.clone(),
            )))
            .footer(footer)
    }
}

pub(crate) fn ensure_left_panel(
    app: Entity<WgApp>,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Entity<LeftPanelState> {
    let app_handle = app.clone();
    window.use_keyed_state("left-panel-entity", cx, move |_, cx| {
        LeftPanelState::new(app_handle.clone(), cx)
    })
}

pub(crate) fn sync_left_panel(
    panel: &Entity<LeftPanelState>,
    app: &WgApp,
    cx: &mut Context<WgApp>,
) {
    let draft_dirty = app
        .ui
        .configs_workspace
        .as_ref()
        .is_some_and(|workspace| workspace.read(cx).draft.is_dirty());
    let next_key = LeftPanelCacheKey {
        active: app.ui_session.sidebar_active,
        sidebar_collapsed: app.ui_session.sidebar_collapsed,
        runtime_running: app.runtime.running,
        runtime_busy: app.runtime.busy,
        quantum_protected: app.runtime.quantum_protected,
        running_name: app.runtime.running_name.clone(),
        draft_dirty,
    };
    let status_text: SharedString = if app.runtime.busy {
        "Updating".into()
    } else if let Some(name) = app.runtime.running_name.as_ref() {
        if app.runtime.quantum_protected {
            format!("{name} - Quantum").into()
        } else {
            name.clone().into()
        }
    } else {
        "Ready".into()
    };
    let active = app.ui_session.sidebar_active;
    let runtime_running = app.runtime.running;
    let next_snapshot = LeftPanelSnapshot {
        sidebar_collapsed: app.ui_session.sidebar_collapsed,
        runtime_running,
        quantum_protected: app.runtime.quantum_protected,
        status_text,
        primary: primary_nav_entries(active, draft_dirty, runtime_running),
        network: insight_nav_entries(active),
        footer: vec![
            NavEntry::item(SidebarItem::Advanced, active == SidebarItem::Advanced),
            NavEntry::item(SidebarItem::About, active == SidebarItem::About),
        ],
    };

    panel.update(cx, move |panel, cx| {
        panel.apply_snapshot(next_key, next_snapshot, cx);
    });
}

pub(crate) fn sidebar_uses_overlay(window: &Window) -> bool {
    window.viewport_size().width < px(SIDEBAR_OVERLAY_BREAKPOINT)
}

pub(crate) fn sync_left_panel_overlay(app: &mut WgApp, window: &Window, cx: &mut Context<WgApp>) {
    if !sidebar_can_present_overlay(window) && app.ui_session.sidebar_overlay_open {
        app.close_sidebar_overlay(cx);
    }
}

pub(crate) fn render_left_panel_overlay(
    panel: &Entity<LeftPanelState>,
    app: &WgApp,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) -> Option<AnyElement> {
    if !app.ui_session.sidebar_overlay_open || !sidebar_can_present_overlay(window) {
        return None;
    }

    let (app_handle, snapshot) = {
        let panel = panel.read(cx);
        let snapshot = panel.snapshot.as_ref()?.clone();
        (panel.app.clone(), snapshot)
    };

    Some(
        div()
            .id("left-panel-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(cx.theme().background.alpha(0.42))
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_sidebar_overlay(cx);
            }))
            .child(
                h_flex().h_full().w_full().child(
                    div()
                        .id("left-panel-overlay-drawer")
                        .h_full()
                        .w(overlay_sheet_width(window))
                        .shadow_xl()
                        .on_click(|_, _, cx| {
                            cx.stop_propagation();
                        })
                        .child(LeftPanelSurface::new(
                            app_handle,
                            snapshot,
                            LeftPanelPresentation::Overlay,
                        )),
                ),
            )
            .into_any_element(),
    )
}

pub(crate) fn toggle_sidebar(app: &mut WgApp, window: &Window, cx: &mut Context<WgApp>) {
    if sidebar_uses_overlay(window) || sidebar_is_force_collapsed(window) {
        if app.ui_session.sidebar_overlay_open {
            app.close_sidebar_overlay(cx);
        } else {
            app.open_sidebar_overlay(cx);
        }
    } else {
        app.toggle_sidebar_collapsed(cx);
    }
}

fn render_sidebar_header(
    snapshot: &LeftPanelSnapshot,
    app_handle: Entity<WgApp>,
    collapsed: bool,
    presentation: LeftPanelPresentation,
    cx: &mut App,
) -> impl IntoElement {
    let toggle = SidebarToggleButton::left()
        .collapsed(collapsed)
        .on_click(move |_, window, cx| {
            app_handle.update(cx, |app, cx| match presentation {
                LeftPanelPresentation::Docked => toggle_sidebar(app, window, cx),
                LeftPanelPresentation::Overlay => app.close_sidebar_overlay(cx),
            });
        });

    if collapsed {
        return div()
            .w_full()
            .flex()
            .justify_center()
            .child(toggle)
            .into_any_element();
    }

    let status_tone = if snapshot.runtime_running {
        cx.theme().sidebar_primary
    } else {
        cx.theme().sidebar_border.alpha(0.9)
    };

    div()
        .w_full()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    h_flex()
                        .items_center()
                        .gap_3()
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
                        .when(!collapsed, |this| {
                            this.child(
                                v_flex()
                                    .gap_0p5()
                                    .overflow_hidden()
                                    .child(div().text_sm().font_semibold().child("r-wg"))
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_1p5()
                                            .overflow_hidden()
                                            .child(
                                                div().size(px(6.0)).rounded_full().bg(status_tone),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .truncate()
                                                    .child(snapshot.status_text.clone()),
                                            )
                                            .when(snapshot.quantum_protected, |this| {
                                                this.child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(cx.theme().sidebar_primary)
                                                        .child("Protected"),
                                                )
                                            }),
                                    ),
                            )
                        }),
                )
                .child(toggle),
        )
        .into_any_element()
}

fn primary_nav_entries(
    active_item: SidebarItem,
    draft_dirty: bool,
    running: bool,
) -> Vec<NavEntry> {
    vec![
        NavEntry::item(SidebarItem::Overview, active_item == SidebarItem::Overview),
        NavEntry::item(SidebarItem::Configs, active_item == SidebarItem::Configs)
            .when(draft_dirty, |entry| {
                entry.suffix("Dirty", NavSuffixTone::Accent)
            }),
        NavEntry::item(SidebarItem::Proxies, active_item == SidebarItem::Proxies)
            .when(running, |entry| entry.suffix("Live", NavSuffixTone::Muted)),
        NavEntry::item(SidebarItem::Dns, active_item == SidebarItem::Dns),
        NavEntry::item(SidebarItem::Logs, active_item == SidebarItem::Logs),
    ]
}

fn insight_nav_entries(active_item: SidebarItem) -> Vec<NavEntry> {
    vec![
        NavEntry::item(SidebarItem::RouteMap, active_item == SidebarItem::RouteMap),
        NavEntry::item(SidebarItem::Tools, active_item == SidebarItem::Tools),
        NavEntry::group(
            "coming-soon",
            "Coming Soon",
            IconName::Ellipsis,
            vec![
                NavEntry::item(SidebarItem::Connections, false).disabled(true),
                NavEntry::item(SidebarItem::Providers, false).disabled(true),
                NavEntry::item(SidebarItem::Rules, false).disabled(true),
                NavEntry::item(SidebarItem::TrafficStats, false)
                    .suffix("Soon", NavSuffixTone::Muted)
                    .disabled(true),
                NavEntry::item(SidebarItem::Topology, false)
                    .suffix("Beta", NavSuffixTone::Muted)
                    .disabled(true),
            ],
            false,
        ),
    ]
}

fn render_nav_entry(
    entry: NavEntry,
    collapsed: bool,
    app: Entity<WgApp>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let has_children = !entry.children.is_empty();
    let is_clickable = !entry.disabled && (entry.item.is_some() || (has_children && !collapsed));
    let item = entry.item;
    let open_state = has_children.then(|| {
        window.use_keyed_state(entry.key, cx, move |_, _| {
            entry.default_open || entry.active
        })
    });
    let is_open = open_state
        .as_ref()
        .is_some_and(|state| !collapsed && *state.read(cx));
    let tooltip = entry.label.to_string();
    let active = entry.active;
    let disabled = entry.disabled;
    let icon = entry.icon;
    let label = entry.label;
    let suffix = entry.suffix.clone();

    let row = div()
        .id(entry.key)
        .relative()
        .w_full()
        .child(
            h_flex()
                .w_full()
                .h(px(38.0))
                .px_2()
                .gap_2()
                .items_center()
                .rounded(px(11.0))
                .border_1()
                .border_color(if active {
                    cx.theme().sidebar_primary.alpha(0.28)
                } else {
                    transparent_black()
                })
                .bg(if active {
                    cx.theme().sidebar_accent
                } else {
                    transparent_black()
                })
                .text_color(if disabled {
                    cx.theme().muted_foreground
                } else if active {
                    cx.theme().sidebar_accent_foreground
                } else {
                    cx.theme().sidebar_foreground
                })
                .when(is_clickable, |this| {
                    this.hover(|this| {
                        this.bg(cx.theme().sidebar_accent.opacity(0.72))
                            .text_color(cx.theme().sidebar_accent_foreground)
                    })
                })
                .when(collapsed, |this| this.justify_center().px_0())
                .when(!collapsed, |this| {
                    this.child(div().w(px(2.0)).h_4().rounded_full().bg(if active {
                        cx.theme().sidebar_primary
                    } else {
                        transparent_black()
                    }))
                })
                .child(Icon::new(icon).size_4().text_color(if active {
                    cx.theme().sidebar_accent_foreground
                } else if disabled {
                    cx.theme().muted_foreground
                } else {
                    cx.theme().sidebar_foreground.opacity(0.9)
                }))
                .when(!collapsed, |this| {
                    this.child(
                        h_flex()
                            .flex_1()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .overflow_hidden()
                            .child(div().flex_1().truncate().text_sm().child(label))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .when_some(suffix, |this, suffix| {
                                        this.child(render_nav_suffix(suffix, active, cx))
                                    })
                                    .when(has_children, |this| {
                                        this.child(
                                            Icon::new(IconName::ChevronRight).size_3().rotate(
                                                if is_open {
                                                    percentage(90.0 / 360.0)
                                                } else {
                                                    percentage(0.0)
                                                },
                                            ),
                                        )
                                    }),
                            ),
                    )
                }),
        )
        .when(is_clickable, |this| {
            let app = app.clone();
            let open_state = open_state.clone();
            this.on_click(move |_, window, cx| {
                if let Some(item) = item {
                    app.update(cx, |app, cx| {
                        activate_sidebar_item(app, item, window, cx);
                    });
                } else if let Some(open_state) = open_state.as_ref() {
                    open_state.update(cx, |is_open, cx| {
                        *is_open = !*is_open;
                        cx.notify();
                    });
                }
            })
        })
        .when(collapsed, |this| {
            this.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        });

    if has_children && !collapsed && is_open {
        v_flex()
            .gap_1()
            .child(row)
            .child(
                v_flex()
                    .gap_1()
                    .ml_4()
                    .pl_3()
                    .border_l_1()
                    .border_color(cx.theme().sidebar_border.alpha(0.5))
                    .children(
                        entry.children.into_iter().map(|child| {
                            render_nav_entry(child, collapsed, app.clone(), window, cx)
                        }),
                    ),
            )
            .into_any_element()
    } else {
        row.into_any_element()
    }
}

fn render_nav_suffix(suffix: NavSuffix, active: bool, cx: &mut App) -> impl IntoElement {
    match suffix {
        NavSuffix::Label(label, tone) => {
            let background = match tone {
                NavSuffixTone::Muted => {
                    if active {
                        cx.theme().sidebar_accent_foreground.opacity(0.14)
                    } else {
                        cx.theme().sidebar_border.alpha(0.22)
                    }
                }
                NavSuffixTone::Accent => {
                    if active {
                        cx.theme().sidebar_accent_foreground.opacity(0.16)
                    } else {
                        cx.theme().sidebar_primary.alpha(0.14)
                    }
                }
            };
            let foreground = match tone {
                NavSuffixTone::Muted => {
                    if active {
                        cx.theme().sidebar_accent_foreground
                    } else {
                        cx.theme().muted_foreground
                    }
                }
                NavSuffixTone::Accent => {
                    if active {
                        cx.theme().sidebar_accent_foreground
                    } else {
                        cx.theme().sidebar_primary
                    }
                }
            };

            div()
                .px_1p5()
                .h_5()
                .rounded_full()
                .flex()
                .items_center()
                .bg(background)
                .text_color(foreground)
                .text_xs()
                .child(label)
        }
    }
}

fn sidebar_is_collapsed(collapsed_pref: bool, window: &Window) -> bool {
    collapsed_pref || sidebar_is_force_collapsed(window)
}

fn sidebar_is_force_collapsed(window: &Window) -> bool {
    window.viewport_size().width < px(SIDEBAR_FORCE_COLLAPSE_BREAKPOINT)
}

fn sidebar_can_present_overlay(window: &Window) -> bool {
    sidebar_uses_overlay(window) || sidebar_is_force_collapsed(window)
}

fn activate_sidebar_item(
    app: &mut WgApp,
    item: SidebarItem,
    window: &mut Window,
    cx: &mut Context<WgApp>,
) {
    if app.ui_session.sidebar_active == item {
        if app.ui_session.sidebar_overlay_open {
            app.close_sidebar_overlay(cx);
        }
        return;
    }

    app.command_open_sidebar_item(item, window, cx);

    if app.ui_session.sidebar_overlay_open && app.ui_session.sidebar_active == item {
        app.close_sidebar_overlay(cx);
    }
}

fn overlay_sheet_width(window: &Window) -> Pixels {
    let viewport_width = window.viewport_size().width;
    viewport_width.min(px(SIDEBAR_OVERLAY_WIDTH))
}

trait NavEntryBuilderExt {
    fn when(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self
    where
        Self: Sized;
}

impl NavEntryBuilderExt for NavEntry {
    fn when(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self {
        if condition {
            then(self)
        } else {
            self
        }
    }
}
